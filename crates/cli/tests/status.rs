mod common;

#[test]
fn test_status_shows_embedding_info() {
    let env = common::TestEnv::new();
    let output = env
        .doxus_cmd()
        .args(["status"])
        .assert()
        .success()
        .get_output()
        .clone();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Embedding:"),
        "status should show Embedding line, got: {}",
        stdout
    );
}
