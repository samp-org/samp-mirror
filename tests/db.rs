use samp_mirror::db::{Db, InsertRemark};

fn temp_db() -> (Db, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    let db = Db::open(path.to_str().unwrap());
    (db, dir)
}

#[test]
fn test_open_creates_tables() {
    let (_db, _dir) = temp_db();
}

#[test]
fn test_last_block_empty() {
    let (db, _dir) = temp_db();
    assert_eq!(db.last_block(), 0);
}

#[test]
fn test_insert_and_query_remark_by_type() {
    let (db, _dir) = temp_db();
    db.insert_remark(&InsertRemark {
        block_number: 100,
        ext_index: 1,
        sender: "5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY",
        content_type: 0x10,
        channel_block: None,
        channel_index: None,
    });
    let results = db.remarks_by_type(0x10, 0);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].block, 100);
    assert_eq!(results[0].index, 1);
}

#[test]
fn test_insert_and_query_remark_by_sender() {
    let (db, _dir) = temp_db();
    let sender = "5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY";
    db.insert_remark(&InsertRemark {
        block_number: 200,
        ext_index: 3,
        sender,
        content_type: 0x11,
        channel_block: None,
        channel_index: None,
    });
    let results = db.remarks_by_sender(sender, 0);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].block, 200);
    assert_eq!(results[0].index, 3);

    let empty = db.remarks_by_sender("5FHneW46xGXgs5mUiveU4sbTyGBzmstUspZC92UhjJM694ty", 0);
    assert!(empty.is_empty());
}

#[test]
fn test_query_remark_after_filter() {
    let (db, _dir) = temp_db();
    let sender = "5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY";
    db.insert_remark(&InsertRemark {
        block_number: 10,
        ext_index: 0,
        sender,
        content_type: 0x10,
        channel_block: None,
        channel_index: None,
    });
    db.insert_remark(&InsertRemark {
        block_number: 20,
        ext_index: 0,
        sender,
        content_type: 0x10,
        channel_block: None,
        channel_index: None,
    });

    let results = db.remarks_by_type(0x10, 15);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].block, 20);
}

#[test]
fn test_insert_and_query_channel() {
    let (db, _dir) = temp_db();
    db.insert_channel(500, 2);
    let channels = db.channels();
    assert_eq!(channels.len(), 1);
    assert_eq!(channels[0].block, 500);
    assert_eq!(channels[0].index, 2);
}

#[test]
fn test_channel_messages() {
    let (db, _dir) = temp_db();
    db.insert_channel(100, 2);
    // content_type 0x14 == channel message (0x10 | 0x04)
    db.insert_remark(&InsertRemark {
        block_number: 150,
        ext_index: 1,
        sender: "5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY",
        content_type: 20, // decimal for 0x14
        channel_block: Some(100),
        channel_index: Some(2),
    });
    let msgs = db.channel_messages(100, 2, 0);
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].block, 150);
    assert_eq!(msgs[0].index, 1);
}

#[test]
fn test_channel_messages_after_filter() {
    let (db, _dir) = temp_db();
    db.insert_channel(100, 2);
    db.insert_remark(&InsertRemark {
        block_number: 10,
        ext_index: 0,
        sender: "5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY",
        content_type: 20,
        channel_block: Some(100),
        channel_index: Some(2),
    });
    db.insert_remark(&InsertRemark {
        block_number: 20,
        ext_index: 0,
        sender: "5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY",
        content_type: 20,
        channel_block: Some(100),
        channel_index: Some(2),
    });

    let msgs = db.channel_messages(100, 2, 15);
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].block, 20);
}

#[test]
fn test_last_block_after_inserts() {
    let (db, _dir) = temp_db();
    let sender = "5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY";
    for (block, idx) in [(5, 0), (10, 0), (15, 0)] {
        db.insert_remark(&InsertRemark {
            block_number: block,
            ext_index: idx,
            sender,
            content_type: 0x10,
            channel_block: None,
            channel_index: None,
        });
    }
    assert_eq!(db.last_block(), 15);
}
