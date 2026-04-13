use std::process::Command;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_samp-mirror"))
}

#[test]
fn test_cli_snapshot_success() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let output = dir.path().join("snapshot.tar.gz");

    let db = samp_mirror::db::Db::open(db_path.to_str().unwrap());
    db.insert_remark(&samp_mirror::db::InsertRemark {
        block_number: 1,
        ext_index: 0,
        sender: "5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY",
        content_type: 0x10,
        channel_block: None,
        channel_index: None,
    });
    drop(db);

    let output_cmd = bin()
        .args([
            "snapshot",
            "--db",
            db_path.to_str().unwrap(),
            "--output",
            output.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(output_cmd.status.success());
    assert!(output.exists());
}

#[test]
fn test_cli_snapshot_missing_db() {
    let dir = tempfile::tempdir().unwrap();
    let output = bin()
        .args([
            "snapshot",
            "--db",
            dir.path().join("nope.db").to_str().unwrap(),
            "--output",
            "out.tar.gz",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
}

#[test]
fn test_cli_no_node_exits() {
    let output = bin().output().unwrap();
    assert!(!output.status.success());
}
