//! Teardown reaches grandchildren and refuses to signal unsafe pgids
//! (T-07-01, T-07-02).

#[allow(dead_code)]
mod common;

use std::time::Duration;

use rsg_bench::{client, procs};

/// `--spawn-child`: the stub has a grandchild inheriting its process group.
/// `teardown` must reach it too, not just the direct child.
#[tokio::test(flavor = "multi_thread")]
async fn teardown_kills_grandchild() {
    let port = common::free_port();
    let log_path = common::unique_log_path();
    let spec = common::stub_spec(port, &log_path, &["--spawn-child"]);

    let handle = procs::launch(&spec, port).expect("launch bench-stub");
    let client = client::build_client().expect("build client");
    procs::wait_ready(
        &handle,
        &client,
        port,
        Duration::from_secs(10),
        Duration::from_millis(50),
    )
    .await
    .expect("bench-stub became ready");

    // Give the grandchild a moment to actually spawn.
    tokio::time::sleep(Duration::from_millis(200)).await;
    let before = procs::group_members_alive(handle.pgid);
    assert_eq!(
        before.len(),
        2,
        "expected stub + spawned child, got {before:?}"
    );

    let report = procs::teardown(handle, Duration::from_secs(5))
        .await
        .expect("teardown");
    assert!(
        report.survivors.is_empty(),
        "survivors: {:?}",
        report.survivors
    );
}

/// T-07-01: `check_signalable_pgid` must refuse pgid 0, pgid 1, and the
/// harness's own process group.
#[test]
fn refuses_unsafe_pgids() {
    assert!(
        procs::check_signalable_pgid(0).is_err(),
        "pgid 0 must be refused"
    );
    assert!(
        procs::check_signalable_pgid(1).is_err(),
        "pgid 1 must be refused"
    );
    let own = nix::unistd::getpgrp().as_raw();
    assert!(
        procs::check_signalable_pgid(own).is_err(),
        "the harness's own pgid {own} must be refused"
    );
}
