mod common;

use predicates::prelude::*;

#[test]
fn test_empty_query_exits_with_message() {
    let env = common::TestEnv::new();
    env.doxus_cmd()
        .args(["search", ""])
        .assert()
        .failure()
        .code(2)
        .stderr(predicate::str::contains("must not be empty"));
}

#[test]
fn test_whitespace_only_query_exits() {
    let env = common::TestEnv::new();
    env.doxus_cmd()
        .args(["search", "   "])
        .assert()
        .failure()
        .code(2)
        .stderr(predicate::str::contains("must not be empty"));
}
