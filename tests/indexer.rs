use samp_mirror::db::Db;
use serde_json::json;
use std::sync::Arc;
use tokio::sync::Mutex;

fn temp_db() -> (Arc<Mutex<Db>>, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    let db = Db::open(path.to_str().unwrap());
    (Arc::new(Mutex::new(db)), dir)
}

fn build_extrinsic(
    pubkey: &samp::Pubkey,
    pallet: u8,
    call: u8,
    remark_payload: &[u8],
) -> String {
    let mut call_args = Vec::new();
    samp::encode_compact(remark_payload.len() as u64, &mut call_args);
    call_args.extend_from_slice(remark_payload);

    let chain_params = samp::ChainParams::new(
        samp::GenesisHash::from_bytes([0u8; 32]),
        samp::SpecVersion::new(1),
        samp::TxVersion::new(1),
    );

    let ext = samp::build_signed_extrinsic(
        samp::PalletIdx::new(pallet),
        samp::CallIdx::new(call),
        &samp::CallArgs::from_bytes(call_args),
        pubkey,
        |_| samp::Signature::from_bytes([0u8; 64]),
        samp::ExtrinsicNonce::ZERO,
        &chain_params,
    )
    .unwrap();

    format!("0x{}", hex::encode(ext.as_bytes()))
}

fn build_samp_extrinsic(pubkey: &samp::Pubkey, remark_payload: &[u8]) -> String {
    build_extrinsic(pubkey, 0, 7, remark_payload)
}

fn test_pubkey() -> samp::Pubkey {
    let mut bytes = [0u8; 32];
    bytes[0] = 0xd4; // Alice-ish
    bytes[1] = 0x35;
    samp::Pubkey::from_bytes(bytes)
}

#[tokio::test]
async fn test_process_block_with_samp_remark() {
    let (db, _dir) = temp_db();
    let pubkey = test_pubkey();

    // content_type 0x10 (Public) + 32-byte recipient + body
    let mut remark = vec![0x10];
    remark.extend_from_slice(&[0u8; 32]); // recipient
    remark.extend_from_slice(b"hello");

    let ext_hex = build_samp_extrinsic(&pubkey, &remark);
    let block = json!({
        "header": { "number": "0x64" },
        "extrinsics": ["0x00", ext_hex]
    });

    samp_mirror::indexer::process_block(&block, 100, &db, 42).await;

    let db_lock = db.lock().await;
    let results = db_lock.remarks_by_type(0x10, 0);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].block, 100);
    assert_eq!(results[0].index, 1);
}

#[tokio::test]
async fn test_process_block_without_samp_remarks() {
    let (db, _dir) = temp_db();
    let pubkey = test_pubkey();

    // Non-SAMP remark: first byte 0x00 fails is_samp_remark (needs 0xF0 mask == 0x10)
    let remark = vec![0x00, 0x01, 0x02];
    let ext_hex = build_samp_extrinsic(&pubkey, &remark);
    let block = json!({
        "header": { "number": "0x0a" },
        "extrinsics": [ext_hex]
    });

    samp_mirror::indexer::process_block(&block, 10, &db, 42).await;

    let db_lock = db.lock().await;
    assert_eq!(db_lock.last_block(), 0);
}

#[tokio::test]
async fn test_process_block_skips_unsigned() {
    let (db, _dir) = temp_db();

    // An unsigned extrinsic: just raw bytes without 0x84 signed prefix.
    // extract_signer checks that the first payload byte has bit 0x80 set.
    // SCALE compact prefix 0x04 means length 1, then payload byte 0x00 => unsigned.
    let block = json!({
        "header": { "number": "0x0a" },
        "extrinsics": ["0x0400"]
    });

    samp_mirror::indexer::process_block(&block, 10, &db, 42).await;

    let db_lock = db.lock().await;
    assert_eq!(db_lock.last_block(), 0);
}

#[tokio::test]
async fn test_process_block_channel_create() {
    let (db, _dir) = temp_db();
    let pubkey = test_pubkey();

    // content_type 0x13 (ChannelCreate) + name_len + name + desc_len + desc
    let name = b"general";
    let desc = b"a channel";
    let mut remark = vec![0x13];
    remark.push(name.len() as u8);
    remark.extend_from_slice(name);
    remark.push(desc.len() as u8);
    remark.extend_from_slice(desc);

    let ext_hex = build_samp_extrinsic(&pubkey, &remark);
    let block = json!({
        "header": { "number": "0xc8" },
        "extrinsics": [ext_hex]
    });

    samp_mirror::indexer::process_block(&block, 200, &db, 42).await;

    let db_lock = db.lock().await;
    let channels = db_lock.channels();
    assert_eq!(channels.len(), 1);
    assert_eq!(channels[0].block, 200);
    assert_eq!(channels[0].index, 0);
}

#[tokio::test]
async fn test_process_block_channel_message() {
    let (db, _dir) = temp_db();
    let pubkey = test_pubkey();

    // content_type 0x14 (Channel) + channel_block(4 LE) + channel_index(2 LE) + ...
    let mut remark = vec![0x14];
    remark.extend_from_slice(&100u32.to_le_bytes()); // channel_block
    remark.extend_from_slice(&2u16.to_le_bytes());   // channel_index
    // reply_to + continues block refs (6 bytes each)
    remark.extend_from_slice(&[0u8; 6]); // reply_to
    remark.extend_from_slice(&[0u8; 6]); // continues
    remark.extend_from_slice(b"msg");

    let ext_hex = build_samp_extrinsic(&pubkey, &remark);
    let block = json!({
        "header": { "number": "0x12c" },
        "extrinsics": [ext_hex]
    });

    samp_mirror::indexer::process_block(&block, 300, &db, 42).await;

    let db_lock = db.lock().await;
    let msgs = db_lock.channel_messages(100, 2, 0);
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].block, 300);
}

#[tokio::test]
async fn test_process_block_no_extrinsics_field() {
    let (db, _dir) = temp_db();
    let block = json!({"header": {"number": "0x01"}});
    samp_mirror::indexer::process_block(&block, 1, &db, 42).await;
    assert_eq!(db.lock().await.last_block(), 0);
}

#[tokio::test]
async fn test_process_block_exceeds_u32_max() {
    let (db, _dir) = temp_db();
    let block = json!({"extrinsics": ["0x00"]});
    samp_mirror::indexer::process_block(&block, u64::MAX, &db, 42).await;
    assert_eq!(db.lock().await.last_block(), 0);
}

#[tokio::test]
async fn test_process_block_invalid_hex_extrinsic() {
    let (db, _dir) = temp_db();
    let block = json!({"extrinsics": ["not-valid-hex"]});
    samp_mirror::indexer::process_block(&block, 1, &db, 42).await;
    assert_eq!(db.lock().await.last_block(), 0);
}

#[tokio::test]
async fn test_process_block_non_string_extrinsic() {
    let (db, _dir) = temp_db();
    let block = json!({"extrinsics": [42, null, true]});
    samp_mirror::indexer::process_block(&block, 1, &db, 42).await;
    assert_eq!(db.lock().await.last_block(), 0);
}

#[tokio::test]
async fn test_process_block_wrong_pallet() {
    let (db, _dir) = temp_db();
    let pubkey = test_pubkey();
    let mut remark = vec![0x10];
    remark.extend_from_slice(&[0u8; 32]);
    remark.extend_from_slice(b"wrong pallet");
    let ext_hex = build_extrinsic(&pubkey, 5, 7, &remark);
    let block = json!({"extrinsics": [ext_hex]});
    samp_mirror::indexer::process_block(&block, 1, &db, 42).await;
    assert_eq!(db.lock().await.last_block(), 0);
}

#[tokio::test]
async fn test_process_block_wrong_call_index() {
    let (db, _dir) = temp_db();
    let pubkey = test_pubkey();
    let mut remark = vec![0x10];
    remark.extend_from_slice(&[0u8; 32]);
    remark.extend_from_slice(b"wrong call");
    let ext_hex = build_extrinsic(&pubkey, 0, 3, &remark);
    let block = json!({"extrinsics": [ext_hex]});
    samp_mirror::indexer::process_block(&block, 1, &db, 42).await;
    assert_eq!(db.lock().await.last_block(), 0);
}

#[tokio::test]
async fn test_process_block_empty_extrinsics() {
    let (db, _dir) = temp_db();
    let block = json!({"extrinsics": []});
    samp_mirror::indexer::process_block(&block, 1, &db, 42).await;
    assert_eq!(db.lock().await.last_block(), 0);
}

#[tokio::test]
async fn test_process_block_invalid_ss58_prefix() {
    let (db, _dir) = temp_db();
    let pubkey = test_pubkey();
    let mut remark = vec![0x10];
    remark.extend_from_slice(&[0u8; 32]);
    remark.extend_from_slice(b"bad prefix");
    let ext_hex = build_samp_extrinsic(&pubkey, &remark);
    let block = json!({"extrinsics": [ext_hex]});
    samp_mirror::indexer::process_block(&block, 1, &db, 16384).await;
    assert_eq!(db.lock().await.last_block(), 0);
}

#[tokio::test]
async fn test_process_block_remark_with_event() {
    let (db, _dir) = temp_db();
    let pubkey = test_pubkey();
    let mut remark = vec![0x10];
    remark.extend_from_slice(&[0u8; 32]);
    remark.extend_from_slice(b"via remark_with_event");
    // call index 9 = remark_with_event
    let ext_hex = build_extrinsic(&pubkey, 0, 9, &remark);
    let block = json!({"extrinsics": [ext_hex]});
    samp_mirror::indexer::process_block(&block, 50, &db, 42).await;
    let db_lock = db.lock().await;
    assert_eq!(db_lock.remarks_by_type(0x10, 0).len(), 1);
    assert_eq!(db_lock.remarks_by_type(0x10, 0)[0].block, 50);
}
