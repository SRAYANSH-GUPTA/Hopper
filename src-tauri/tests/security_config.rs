use serde_json::Value;

#[test]
fn packaged_renderer_restricts_scripts_and_embedded_content() {
    let config: Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
    let csp = config["app"]["security"]["csp"]
        .as_object()
        .expect("packaged app must have a content security policy");
    let scripts: Vec<_> = csp["script-src"]
        .as_str()
        .unwrap()
        .split_whitespace()
        .collect();
    assert!(scripts.contains(&"'self'"));
    assert!(
        scripts.contains(&"'wasm-unsafe-eval'"),
        "diff syntax highlighting uses WebAssembly"
    );
    for forbidden in [
        "'unsafe-inline'",
        "'unsafe-eval'",
        "*",
        "http:",
        "https:",
        "data:",
    ] {
        assert!(
            !scripts.contains(&forbidden),
            "packaged scripts must not allow {forbidden}"
        );
    }
    assert_eq!(csp["object-src"], "'none'");
    assert_eq!(csp["frame-src"], "'none'");
    assert_eq!(csp["base-uri"], "'self'");
    assert_eq!(csp["form-action"], "'none'");
    assert!(!csp["connect-src"]
        .as_str()
        .unwrap()
        .split_whitespace()
        .any(|source| source == "ws:"));
}
