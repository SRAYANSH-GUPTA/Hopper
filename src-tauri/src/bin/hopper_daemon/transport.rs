use super::rpc::{
    build_error_response, build_result_response, forward_events, parse_auth_token,
    spawn_rpc_response_task,
};
use super::*;
use futures_util::future::{select, Either};
use std::io;
use std::time::Duration;
use tokio::io::{AsyncBufRead, AsyncReadExt};

const MAX_AUTH_FRAME_BYTES: usize = 16 * 1024;
const MAX_RPC_FRAME_BYTES: usize = 32 * 1024 * 1024;
const MAX_QUEUED_RESPONSES: usize = 64;
const AUTH_TIMEOUT: Duration = Duration::from_secs(10);
const WRITE_TIMEOUT: Duration = Duration::from_secs(30);

// Bound the read itself, including a peer that never sends a newline. Checking
// length after `lines().next_line()` would already have allocated its payload.
async fn read_frame<R: AsyncBufRead + Unpin>(
    reader: &mut R,
    limit: usize,
) -> io::Result<Option<Vec<u8>>> {
    let mut frame = Vec::new();
    (&mut *reader)
        .take(limit as u64 + 1)
        .read_until(b'\n', &mut frame)
        .await?;
    if frame.is_empty() {
        return Ok(None);
    }
    if frame.last() == Some(&b'\n') {
        frame.pop();
    }
    if frame.len() > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "RPC frame exceeds size limit",
        ));
    }
    Ok(Some(frame))
}

pub(super) async fn handle_client(
    socket: TcpStream,
    config: Arc<DaemonConfig>,
    state: Arc<DaemonState>,
    events: broadcast::Sender<DaemonEvent>,
) {
    let (reader, mut writer) = socket.into_split();
    let mut reader = BufReader::new(reader);

    let (out_tx, mut out_rx) = mpsc::channel::<String>(MAX_QUEUED_RESPONSES);
    let write_task = tokio::spawn(async move {
        while let Some(message) = out_rx.recv().await {
            let write = async {
                writer.write_all(message.as_bytes()).await?;
                writer.write_all(b"\n").await
            };
            if !matches!(tokio::time::timeout(WRITE_TIMEOUT, write).await, Ok(Ok(()))) {
                break;
            }
        }
    });

    let mut authenticated = config.token.is_none();
    let auth_deadline = tokio::time::Instant::now() + AUTH_TIMEOUT;
    let mut auth_failures = 0;
    let mut events_task: Option<tokio::task::JoinHandle<()>> = None;
    let request_limiter = Arc::new(Semaphore::new(MAX_IN_FLIGHT_RPC_PER_CONNECTION));
    let client_version = format!("daemon-{}", env!("CARGO_PKG_VERSION"));

    if authenticated {
        let rx = events.subscribe();
        let out_tx_events = out_tx.clone();
        events_task = Some(tokio::spawn(forward_events(rx, out_tx_events)));
    }

    loop {
        let read = async {
            if authenticated {
                read_frame(&mut reader, MAX_RPC_FRAME_BYTES).await
            } else {
                tokio::time::timeout_at(
                    auth_deadline,
                    read_frame(&mut reader, MAX_AUTH_FRAME_BYTES),
                )
                .await
                .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "Authentication timed out"))?
            }
        };
        // Stop reading if the socket writer has failed or its deadline elapsed.
        let line = match select(Box::pin(read), Box::pin(out_tx.closed())).await {
            Either::Left((Ok(Some(line)), _)) => line,
            _ => break,
        };
        let message: Value = match serde_json::from_slice(&line) {
            Ok(value) => value,
            Err(_) => continue,
        };

        let id = message.get("id").and_then(|value| value.as_u64());
        let method = message
            .get("method")
            .and_then(|value| value.as_str())
            .unwrap_or("")
            .to_string();
        let params = message.get("params").cloned().unwrap_or(Value::Null);

        if !authenticated {
            auth_failures += 1;
            if auth_failures > 3 {
                break;
            }
            if method != "auth" {
                if let Some(response) = build_error_response(id, "unauthorized") {
                    let _ = out_tx.send(response).await;
                }
                continue;
            }

            let expected = config.token.clone().unwrap_or_default();
            let provided = parse_auth_token(&params).unwrap_or_default();
            if expected != provided {
                if let Some(response) = build_error_response(id, "invalid token") {
                    let _ = out_tx.send(response).await;
                }
                continue;
            }

            authenticated = true;
            if let Some(response) = build_result_response(id, json!({ "ok": true })) {
                let _ = out_tx.send(response).await;
            }

            let rx = events.subscribe();
            let out_tx_events = out_tx.clone();
            events_task = Some(tokio::spawn(forward_events(rx, out_tx_events)));

            continue;
        }

        spawn_rpc_response_task(
            Arc::clone(&state),
            out_tx.clone(),
            id,
            method,
            params,
            client_version.clone(),
            Arc::clone(&request_limiter),
        )
        .await;
    }

    drop(out_tx);
    if let Some(task) = events_task {
        task.abort();
    }
    write_task.abort();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_async(future: impl std::future::Future<Output = ()>) {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(future);
    }

    #[test]
    fn frame_limit_preserves_boundaries_and_accepts_exact_limit() {
        run_async(async {
            let mut reader = BufReader::new(&b"12345678\nnext\n"[..]);
            assert_eq!(
                read_frame(&mut reader, 8).await.unwrap().unwrap(),
                b"12345678"
            );
            assert_eq!(read_frame(&mut reader, 8).await.unwrap().unwrap(), b"next");
            assert!(read_frame(&mut reader, 8).await.unwrap().is_none());
        });
    }

    #[test]
    fn frame_limit_rejects_oversized_input_before_newline_or_eof() {
        run_async(async {
            let (mut sender, receiver) = tokio::io::duplex(32);
            sender.write_all(b"123456789").await.unwrap();
            let mut reader = BufReader::new(receiver);
            let error = tokio::time::timeout(Duration::from_secs(1), read_frame(&mut reader, 8))
                .await
                .expect("must not wait for sender to close")
                .unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::InvalidData);
            drop(sender);
        });
    }

    fn test_config() -> DaemonConfig {
        DaemonConfig {
            listen: "127.0.0.1:0".parse().unwrap(),
            token: Some("test-secret".into()),
            data_dir: std::env::temp_dir()
                .join(format!("hopper-transport-{}", uuid::Uuid::new_v4())),
        }
    }

    #[test]
    fn saturated_rpc_limit_returns_an_error_without_spawning_waiting_work() {
        run_async(async {
            let config = test_config();
            let (events, _) = broadcast::channel(4);
            let state =
                Arc::new(DaemonState::load(&config, DaemonEventSink { tx: events }).unwrap());
            let (tx, mut rx) = mpsc::channel(1);
            let limiter = Arc::new(Semaphore::new(0));
            spawn_rpc_response_task(
                state,
                tx,
                Some(7),
                "ping".into(),
                Value::Null,
                "test".into(),
                limiter,
            )
            .await;
            let response: Value = serde_json::from_str(&rx.recv().await.unwrap()).unwrap();
            assert_eq!(response["id"], 7);
            assert!(response["error"]["message"]
                .as_str()
                .unwrap()
                .contains("Too many in-flight requests"));
            assert!(rx.recv().await.is_none());
        });
    }

    #[test]
    fn transport_requires_authentication_before_rpc_and_event_subscription() {
        run_async(async {
            let config = Arc::new(test_config());
            let (events, _) = broadcast::channel(4);
            let state = Arc::new(
                DaemonState::load(&config, DaemonEventSink { tx: events.clone() }).unwrap(),
            );
            let listener = TcpListener::bind(config.listen).await.unwrap();
            let mut client = TcpStream::connect(listener.local_addr().unwrap())
                .await
                .unwrap();
            let (server, _) = listener.accept().await.unwrap();
            let task = tokio::spawn(handle_client(server, config, state, events.clone()));
            client
                .write_all(b"{\"id\":1,\"method\":\"ping\"}\n")
                .await
                .unwrap();
            let mut reader = BufReader::new(client);
            let response = read_frame(&mut reader, 1024).await.unwrap().unwrap();
            let response: Value = serde_json::from_slice(&response).unwrap();
            assert_eq!(response["error"]["message"], "unauthorized");
            assert_eq!(events.receiver_count(), 0);

            reader.get_mut().write_all(b"{\"id\":2,\"method\":\"auth\",\"params\":{\"token\":\"test-secret\"}}\n{\"id\":3,\"method\":\"ping\"}\n").await.unwrap();
            for id in [2, 3] {
                let response = read_frame(&mut reader, 1024).await.unwrap().unwrap();
                let response: Value = serde_json::from_slice(&response).unwrap();
                assert_eq!(response["id"], id);
                assert_eq!(response["result"]["ok"], true);
            }
            assert_eq!(events.receiver_count(), 1);
            reader.get_mut().shutdown().await.unwrap();
            tokio::time::timeout(Duration::from_secs(1), task)
                .await
                .unwrap()
                .unwrap();
            tokio::task::yield_now().await;
            assert_eq!(events.receiver_count(), 0);
        });
    }
}
