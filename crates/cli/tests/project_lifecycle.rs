mod common;

use predicates::prelude::*;

#[test]
fn test_disable_missing_project_exits_nonzero() {
    let env = common::TestEnv::new();
    env.doxus_cmd()
        .args(["project", "disable", "nonexistent-project-xyz"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("not found"));
}

#[test]
fn test_enable_missing_project_exits_nonzero() {
    let env = common::TestEnv::new();
    env.doxus_cmd()
        .args(["project", "enable", "nonexistent-project-xyz"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("not found"));
}

#[test]
fn test_remove_missing_project_exits_nonzero() {
    let env = common::TestEnv::new();
    env.doxus_cmd()
        .args(["project", "remove", "nonexistent-project-xyz"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("not found"));
}
