//! Integration test: ProcessKittyClient against a mock `kitten` binary on
//! PATH, mirroring the bash test suite's own approach. Deliberately a
//! single test function, since it mutates process-global env vars (PATH,
//! KITTY_LISTEN_ON), which Rust's default parallel test runner would
//! otherwise race across tests in this same binary.
use std::fs;
use std::os::unix::fs::PermissionsExt;

use kitty_claude_notifier::kitty::{KittyClient, ProcessKittyClient, WindowTarget};

#[test]
fn process_kitty_client_invokes_mock_kitten_correctly() {
    let tmp = tempfile::tempdir().unwrap();
    let log_path = tmp.path().join("calls.log");
    let mock_path = tmp.path().join("kitten");
    fs::write(&mock_path, include_str!("support/mock_kitten.sh")).unwrap();
    let mut perms = fs::metadata(&mock_path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&mock_path, perms).unwrap();

    let original_path = std::env::var("PATH").unwrap_or_default();
    unsafe {
        std::env::set_var("PATH", format!("{}:{original_path}", tmp.path().display()));
        std::env::set_var("MOCK_KITTEN_LOG", &log_path);
        std::env::set_var("MOCK_KITTEN_GET_TEXT", "Do you want to proceed?\n❯ 1. Yes");
        std::env::remove_var("KITTY_LISTEN_ON");
    }

    let client = ProcessKittyClient::new();
    let target = WindowTarget::Id("42".to_string());

    client.set_tab_title(&target, "⛔ Perm").unwrap();
    client.set_tab_color(&target, "#ff003c", "#7a0020").unwrap();
    let text = client.get_text(&target).unwrap();

    let log = fs::read_to_string(&log_path).unwrap();
    assert!(log.contains("set-tab-title --match id:42 ⛔ Perm"));
    assert!(log.contains("set-tab-color --match id:42 active_bg=#ff003c inactive_bg=#7a0020"));
    assert!(log.contains("get-text --match id:42"));
    assert!(text.contains("Do you want to proceed?"));

    unsafe {
        std::env::set_var("PATH", original_path);
        std::env::remove_var("MOCK_KITTEN_LOG");
        std::env::remove_var("MOCK_KITTEN_GET_TEXT");
    }
}
