use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::io::{Read, Write};

fn main() {
    let pty_system = native_pty_system();
    let pair = pty_system.openpty(PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    }).unwrap();

    let mut cmd = CommandBuilder::new("agy");
    cmd.env("TERM", "xterm-256color");
    
    let mut child = pair.slave.spawn_command(cmd).unwrap();
    let mut reader = pair.master.try_clone_reader().unwrap();
    let mut writer = pair.master.take_writer().unwrap();

    std::thread::spawn(move || {
        let mut buffer = [0u8; 4096];
        loop {
            if let Ok(count) = reader.read(&mut buffer) {
                if count == 0 { break; }
                let s = String::from_utf8_lossy(&buffer[..count]);
                print!("{}", s);
            } else {
                break;
            }
        }
    });

    std::thread::sleep(std::time::Duration::from_secs(3));
    for byte in b"/usage" {
        writer.write_all(&[*byte]).unwrap();
        writer.flush().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    std::thread::sleep(std::time::Duration::from_millis(250));
    writer.write_all(b"\r").unwrap();
    writer.flush().unwrap();
    std::thread::sleep(std::time::Duration::from_secs(4));
}
