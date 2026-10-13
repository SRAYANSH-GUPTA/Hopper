use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use serde_json::Value;

#[test]
fn privileged_capabilities_only_target_application_webviews() {
    let capabilities: Value = serde_json::from_str(include_str!("../capabilities/default.json"))
        .expect("capabilities must be valid JSON");
    for capability in capabilities["capabilities"].as_array().unwrap() {
        // Window selectors also grant privileges to embedded remote child views.
        assert!(capability.get("windows").is_none());
        assert!(capability.get("remote").is_none());
        assert!(capability["permissions"]
            .as_array()
            .unwrap()
            .contains(&Value::from("allow-app-commands")));
        let webviews = capability["webviews"].as_array().unwrap();
        assert!(!webviews.is_empty());
        for webview in webviews {
            assert!(matches!(webview.as_str(), Some("main" | "about")));
        }
    }
}

#[test]
fn trusted_app_permission_covers_exactly_the_registered_commands() {
    let permissions = include_str!("../permissions/app.toml")
        .parse::<toml_edit::Document>()
        .expect("app permissions must be valid TOML");
    let app_permission = &permissions["permission"][0];
    assert_eq!(
        app_permission["identifier"].as_str(),
        Some("allow-app-commands")
    );
    let allowed: BTreeSet<_> = app_permission["commands"]["allow"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap())
        .collect();
    let source = include_str!("../src/lib.rs");
    let registered: BTreeSet<_> = source
        .split("tauri::generate_handler![")
        .nth(1)
        .unwrap()
        .split(']')
        .next()
        .unwrap()
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|command| command.rsplit("::").next().unwrap())
        .collect();
    assert_eq!(
        allowed, registered,
        "Update the trusted app permission when registering commands"
    );
    let assistant = &permissions["permission"][1];
    assert_eq!(
        assistant["identifier"].as_str(),
        Some("allow-assistant-transfer")
    );
    let allowed: BTreeSet<_> = assistant["commands"]["allow"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap())
        .collect();
    assert_eq!(
        allowed,
        BTreeSet::from(["web_chat_reply", "web_chat_file_import"])
    );
    assert_eq!(
        permissions["permission"]
            .as_array_of_tables()
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn assistant_capability_only_grants_transfers_on_exact_https_origins() {
    use tauri::utils::acl::RemoteUrlPattern;

    let capability: Value =
        serde_json::from_str(include_str!("../capabilities/assistant-transfer.json")).unwrap();
    assert_eq!(capability["local"], false);
    assert!(capability.get("windows").is_none());
    assert_eq!(capability["webviews"], serde_json::json!(["ai-chatbot-*"]));
    assert_eq!(
        capability["permissions"],
        serde_json::json!(["allow-assistant-transfer"])
    );
    let patterns: Vec<RemoteUrlPattern> = capability["remote"]["urls"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap().parse().unwrap())
        .collect();
    let matches = |url: &str| {
        patterns
            .iter()
            .any(|pattern| pattern.test(&url.parse().unwrap()))
    };
    let hosts = [
        "chatgpt.com",
        "chat.openai.com",
        "claude.ai",
        "gemini.google.com",
        "copilot.microsoft.com",
        "chat.mistral.ai",
    ];
    assert_eq!(patterns.len(), hosts.len() * 2);
    for host in hosts {
        for prefix in ["", "www."] {
            assert!(matches(&format!("https://{prefix}{host}/chat?x=1#reply")));
            assert!(matches(&format!("https://{prefix}{host}:443/")));
            assert!(!matches(&format!("http://{prefix}{host}/")));
            assert!(!matches(&format!("https://{prefix}{host}:8443/")));
            assert!(!matches(&format!("https://{prefix}{host}.example.org/")));
        }
        assert!(!matches(&format!("https://attacker.{host}/")));
    }
    for url in [
        "tauri://localhost",
        "http://tauri.localhost",
        "http://localhost:1420",
        "https://example.org",
        "file:///tmp/index.html",
    ] {
        assert!(!matches(url));
    }
}

#[test]
fn signed_macos_app_declares_provider_login_automation() {
    assert!(include_str!("../Entitlements.plist")
        .contains("<key>com.apple.security.automation.apple-events</key>\n    <true/>"));
    assert!(include_str!("../Info.plist").contains("<key>NSAppleEventsUsageDescription</key>\n    <string>Allow Hopper to open Terminal for provider sign-in.</string>"));
}

#[test]
fn tauri_dependency_requires_channel_isolation_fix() {
    let manifest = include_str!("../Cargo.toml")
        .parse::<toml_edit::Document>()
        .unwrap();
    let minimum: Vec<u32> = manifest["dependencies"]["tauri"]["version"]
        .as_str()
        .unwrap()
        .split('.')
        .map(|part| part.parse().expect("use an explicit Tauri minimum version"))
        .collect();
    assert!(
        minimum.as_slice() >= [2, 11, 6].as_slice(),
        "Tauri must include GHSA-w28w-mhc8-qvjv remediation"
    );
}

#[test]
fn macos_private_api_feature_matches_config() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let config_path = manifest_dir.join("tauri.conf.json");
    let config_contents = fs::read_to_string(&config_path)
        .unwrap_or_else(|error| panic!("Failed to read {config_path:?}: {error}"));
    let config: Value = serde_json::from_str(&config_contents)
        .unwrap_or_else(|error| panic!("Failed to parse tauri.conf.json: {error}"));
    let macos_private_api = config
        .get("app")
        .and_then(|app| app.get("macOSPrivateApi"))
        .and_then(|value| value.as_bool())
        .unwrap_or(false);

    if macos_private_api {
        let cargo_path = manifest_dir.join("Cargo.toml");
        let cargo_contents = fs::read_to_string(&cargo_path)
            .unwrap_or_else(|error| panic!("Failed to read {cargo_path:?}: {error}"));
        let mut in_dependencies = false;
        let mut has_feature = false;

        for line in cargo_contents.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with('[') {
                in_dependencies = trimmed == "[dependencies]";
                continue;
            }
            if !in_dependencies {
                continue;
            }
            if trimmed.starts_with("tauri") && trimmed.contains("macos-private-api") {
                has_feature = true;
                break;
            }
        }

        assert!(
            has_feature,
            "Cargo.toml [dependencies] must enable macos-private-api when app.macOSPrivateApi is true"
        );
    }
}
