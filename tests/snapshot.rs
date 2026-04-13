use samp_mirror::db::{Db, InsertRemark};

#[test]
fn test_snapshot_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("source.db");
    let output_path = dir.path().join("snapshot.tar.gz");

    let db = Db::open(db_path.to_str().unwrap());
    db.insert_remark(&InsertRemark {
        block_number: 100,
        ext_index: 1,
        sender: "5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY",
        content_type: 0x10,
        channel_block: None,
        channel_index: None,
    });
    db.insert_channel(200, 3);
    drop(db);

    let size = samp_mirror::db::snapshot(
        db_path.to_str().unwrap(),
        output_path.to_str().unwrap(),
    )
    .unwrap();
    assert!(size > 0);

    let extract_dir = dir.path().join("extracted");
    let file = std::fs::File::open(&output_path).unwrap();
    let dec = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(dec);
    archive.unpack(&extract_dir).unwrap();

    let extracted_db = Db::open(extract_dir.join("mirror.db").to_str().unwrap());
    assert_eq!(extracted_db.last_block(), 100);
    let channels = extracted_db.channels();
    assert_eq!(channels.len(), 1);
    assert_eq!(channels[0].block, 200);
}

#[test]
fn test_snapshot_missing_db() {
    let dir = tempfile::tempdir().unwrap();
    let result = samp_mirror::db::snapshot(
        dir.path().join("nonexistent.db").to_str().unwrap(),
        dir.path().join("out.tar.gz").to_str().unwrap(),
    );
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Database not found"));
}
