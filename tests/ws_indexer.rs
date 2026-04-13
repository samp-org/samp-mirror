use futures_util::{SinkExt, StreamExt};
use samp_mirror::db::{Db, InsertRemark};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tokio_tungstenite::tungstenite::Message as WsMessage;

fn temp_db() -> (Arc<Mutex<Db>>, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    let db = Db::open(path.to_str().unwrap());
    (Arc::new(Mutex::new(db)), dir)
}

fn test_pubkey() -> samp::Pubkey {
    let mut bytes = [0u8; 32];
    bytes[0] = 0xd4;
    bytes[1] = 0x35;
    samp::Pubkey::from_bytes(bytes)
}

fn build_samp_extrinsic(pubkey: &samp::Pubkey, remark_payload: &[u8]) -> String {
    let mut call_args = Vec::new();
    samp::encode_compact(remark_payload.len() as u64, &mut call_args);
    call_args.extend_from_slice(remark_payload);

    let chain_params = samp::ChainParams::new(
        samp::GenesisHash::from_bytes([0u8; 32]),
        samp::SpecVersion::new(1),
        samp::TxVersion::new(1),
    );

    let ext = samp::build_signed_extrinsic(
        samp::PalletIdx::new(0),
        samp::CallIdx::new(7),
        &samp::CallArgs::from_bytes(call_args),
        pubkey,
        |_| samp::Signature::from_bytes([0u8; 64]),
        samp::ExtrinsicNonce::ZERO,
        &chain_params,
    )
    .unwrap();

    format!("0x{}", hex::encode(ext.as_bytes()))
}

fn samp_remark(body: &[u8]) -> Vec<u8> {
    let mut remark = vec![0x10];
    remark.extend_from_slice(&[0u8; 32]);
    remark.extend_from_slice(body);
    remark
}

// -- Mock Substrate Node --

struct MockNode {
    url: String,
    _handle: tokio::task::JoinHandle<()>,
}

impl Drop for MockNode {
    fn drop(&mut self) {
        self._handle.abort();
    }
}

#[derive(Clone)]
struct MockNodeConfig {
    chain_name: String,
    ss58_prefix: u16,
    head: u64,
    blocks: BTreeMap<u64, Vec<String>>,
    subscription_blocks: Vec<u64>,
    error_on_first_hash: HashSet<u64>,
    send_ping: bool,
}

impl Default for MockNodeConfig {
    fn default() -> Self {
        Self {
            chain_name: "TestChain".into(),
            ss58_prefix: 42,
            head: 0,
            blocks: BTreeMap::new(),
            subscription_blocks: Vec::new(),
            error_on_first_hash: HashSet::new(),
            send_ping: false,
        }
    }
}

async fn start_mock_node(config: MockNodeConfig) -> MockNode {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let url = format!("ws://127.0.0.1:{port}");
    let config = Arc::new(config);

    let handle = tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let config = config.clone();
            tokio::spawn(async move {
                let Ok(ws) = tokio_tungstenite::accept_async(stream).await else {
                    return;
                };
                let (mut write, mut read) = ws.split();
                let errored: std::sync::Mutex<HashSet<u64>> =
                    std::sync::Mutex::new(HashSet::new());
                let ping_sent = AtomicBool::new(false);

                while let Some(Ok(msg)) = read.next().await {
                    let text = match msg {
                        WsMessage::Text(t) => t.to_string(),
                        _ => continue,
                    };
                    let req: Value = match serde_json::from_str(&text) {
                        Ok(v) => v,
                        Err(_) => continue,
                    };
                    let id = req["id"].clone();
                    let method = req["method"].as_str().unwrap_or("");

                    if config.send_ping && !ping_sent.swap(true, Ordering::Relaxed) {
                        let _ = write.send(WsMessage::Ping(vec![].into())).await;
                    }

                    match method {
                        "system_chain" => {
                            let resp = json!({"jsonrpc":"2.0","id":id,"result":config.chain_name});
                            let _ =
                                write.send(WsMessage::Text(resp.to_string().into())).await;
                        }
                        "system_properties" => {
                            let resp = json!({"jsonrpc":"2.0","id":id,"result":{"ss58Format":config.ss58_prefix}});
                            let _ =
                                write.send(WsMessage::Text(resp.to_string().into())).await;
                        }
                        "chain_getHeader" => {
                            let hex = format!("0x{:x}", config.head);
                            let resp =
                                json!({"jsonrpc":"2.0","id":id,"result":{"number":hex}});
                            let _ =
                                write.send(WsMessage::Text(resp.to_string().into())).await;
                        }
                        "chain_getBlockHash" => {
                            let block_num = req["params"][0].as_u64().unwrap_or(0);
                            let should_error = if config
                                .error_on_first_hash
                                .contains(&block_num)
                            {
                                let mut guard = errored.lock().unwrap();
                                if guard.contains(&block_num) {
                                    false
                                } else {
                                    guard.insert(block_num);
                                    true
                                }
                            } else {
                                false
                            };
                            if should_error {
                                let resp = json!({"jsonrpc":"2.0","id":id,"error":{"code":-32000,"message":"rate limited"}});
                                let _ = write
                                    .send(WsMessage::Text(resp.to_string().into()))
                                    .await;
                                continue;
                            }
                            let hash = format!("0x{:064x}", block_num);
                            let resp = json!({"jsonrpc":"2.0","id":id,"result":hash});
                            let _ =
                                write.send(WsMessage::Text(resp.to_string().into())).await;
                        }
                        "chain_getBlock" => {
                            let hash_str = req["params"][0].as_str().unwrap_or("");
                            match u64::from_str_radix(
                                hash_str.trim_start_matches("0x"),
                                16,
                            ) {
                                Ok(block_num) => {
                                    let exts = config
                                        .blocks
                                        .get(&block_num)
                                        .cloned()
                                        .unwrap_or_default();
                                    let hex = format!("0x{:x}", block_num);
                                    let resp = json!({"jsonrpc":"2.0","id":id,"result":{"block":{"header":{"number":hex},"extrinsics":exts}}});
                                    let _ = write
                                        .send(WsMessage::Text(resp.to_string().into()))
                                        .await;
                                }
                                Err(_) => {
                                    let resp =
                                        json!({"jsonrpc":"2.0","id":id,"result":null});
                                    let _ = write
                                        .send(WsMessage::Text(resp.to_string().into()))
                                        .await;
                                }
                            }
                        }
                        "chain_subscribeNewHeads" => {
                            let resp = json!({"jsonrpc":"2.0","id":id,"result":"sub_1"});
                            let _ =
                                write.send(WsMessage::Text(resp.to_string().into())).await;
                            for &block_num in &config.subscription_blocks {
                                let hex = format!("0x{:x}", block_num);
                                let notif = json!({
                                    "jsonrpc":"2.0",
                                    "method":"chain_subscribeNewHeads",
                                    "params":{"subscription":"sub_1","result":{"number":hex}}
                                });
                                let _ = write
                                    .send(WsMessage::Text(notif.to_string().into()))
                                    .await;
                            }
                        }
                        _ => {}
                    }
                }
            });
        }
    });

    MockNode {
        url,
        _handle: handle,
    }
}

async fn wait_for_block(db: &Arc<Mutex<Db>>, target: u64, timeout_secs: u64) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(timeout_secs);
    loop {
        if db.lock().await.last_block() >= target {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting for block {target}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

// -- Tests --

#[tokio::test]
async fn test_fetch_chain_info() {
    let mock = start_mock_node(MockNodeConfig {
        chain_name: "Polkadot".into(),
        ss58_prefix: 0,
        head: 100,
        ..Default::default()
    })
    .await;

    let (chain, prefix) = samp_mirror::indexer::fetch_chain_info(&mock.url)
        .await
        .unwrap();
    assert_eq!(chain, "Polkadot");
    assert_eq!(prefix, 0);
}

#[tokio::test]
async fn test_fetch_chain_info_custom_prefix() {
    let mock = start_mock_node(MockNodeConfig {
        chain_name: "Kusama".into(),
        ss58_prefix: 2,
        head: 50,
        ..Default::default()
    })
    .await;

    let (chain, prefix) = samp_mirror::indexer::fetch_chain_info(&mock.url)
        .await
        .unwrap();
    assert_eq!(chain, "Kusama");
    assert_eq!(prefix, 2);
}

#[tokio::test]
async fn test_fetch_chain_info_connection_refused() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);

    let result =
        samp_mirror::indexer::fetch_chain_info(&format!("ws://127.0.0.1:{port}")).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_fetch_chain_info_with_ping() {
    let mock = start_mock_node(MockNodeConfig {
        chain_name: "PingChain".into(),
        ss58_prefix: 42,
        head: 10,
        send_ping: true,
        ..Default::default()
    })
    .await;

    let (chain, prefix) = samp_mirror::indexer::fetch_chain_info(&mock.url)
        .await
        .unwrap();
    assert_eq!(chain, "PingChain");
    assert_eq!(prefix, 42);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_run_inner_catch_up_and_subscribe() {
    let pubkey = test_pubkey();
    let remark = samp_remark(b"catch-up");

    let mut blocks = BTreeMap::new();
    for i in 1..=5 {
        blocks.insert(i, vec!["0x00".to_string()]);
    }
    blocks.insert(3, vec![build_samp_extrinsic(&pubkey, &remark)]);

    let remark2 = samp_remark(b"subscription");
    blocks.insert(6, vec![build_samp_extrinsic(&pubkey, &remark2)]);

    let mock = start_mock_node(MockNodeConfig {
        head: 5,
        blocks,
        subscription_blocks: vec![6],
        ..Default::default()
    })
    .await;

    let (db, _dir) = temp_db();
    let db2 = db.clone();
    let url = mock.url.clone();
    let handle = tokio::spawn(async move {
        samp_mirror::indexer::run_inner(&url, &db2, 42, 1).await
    });

    wait_for_block(&db, 6, 10).await;
    handle.abort();

    let lock = db.lock().await;
    let remarks = lock.remarks_by_type(0x10, 0);
    assert_eq!(remarks.len(), 2);
    assert_eq!(remarks[0].block, 3);
    assert_eq!(remarks[1].block, 6);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_run_inner_subscription_only() {
    let pubkey = test_pubkey();
    let remark = samp_remark(b"sub-only");

    let mut blocks = BTreeMap::new();
    blocks.insert(101, vec![build_samp_extrinsic(&pubkey, &remark)]);

    let mock = start_mock_node(MockNodeConfig {
        head: 100,
        blocks,
        subscription_blocks: vec![101],
        ..Default::default()
    })
    .await;

    let (db, _dir) = temp_db();
    db.lock().await.insert_remark(&InsertRemark {
        block_number: 100,
        ext_index: 0,
        sender: "5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY",
        content_type: 0x10,
        channel_block: None,
        channel_index: None,
    });

    let db2 = db.clone();
    let url = mock.url.clone();
    let handle = tokio::spawn(async move {
        samp_mirror::indexer::run_inner(&url, &db2, 42, 1).await
    });

    wait_for_block(&db, 101, 10).await;
    handle.abort();

    let lock = db.lock().await;
    let remarks = lock.remarks_by_type(0x10, 100);
    assert_eq!(remarks.len(), 1);
    assert_eq!(remarks[0].block, 101);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_run_inner_resume_from_existing() {
    let pubkey = test_pubkey();
    let remark = samp_remark(b"resumed");

    let mut blocks = BTreeMap::new();
    for i in 51..=55 {
        blocks.insert(i, vec!["0x00".to_string()]);
    }
    blocks.insert(53, vec![build_samp_extrinsic(&pubkey, &remark)]);

    let mock = start_mock_node(MockNodeConfig {
        head: 55,
        blocks,
        ..Default::default()
    })
    .await;

    let (db, _dir) = temp_db();
    db.lock().await.insert_remark(&InsertRemark {
        block_number: 50,
        ext_index: 0,
        sender: "5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY",
        content_type: 0x10,
        channel_block: None,
        channel_index: None,
    });

    let db2 = db.clone();
    let url = mock.url.clone();
    let handle = tokio::spawn(async move {
        samp_mirror::indexer::run_inner(&url, &db2, 42, 1).await
    });

    wait_for_block(&db, 53, 10).await;
    handle.abort();

    let lock = db.lock().await;
    let remarks = lock.remarks_by_type(0x10, 50);
    assert_eq!(remarks.len(), 1);
    assert_eq!(remarks[0].block, 53);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_run_inner_rpc_error_recovery() {
    let pubkey = test_pubkey();
    let remark = samp_remark(b"after-error");

    let mut blocks = BTreeMap::new();
    for i in 1..=5 {
        blocks.insert(i, vec!["0x00".to_string()]);
    }
    blocks.insert(3, vec![build_samp_extrinsic(&pubkey, &remark)]);

    let mock = start_mock_node(MockNodeConfig {
        head: 5,
        blocks,
        error_on_first_hash: HashSet::from([3]),
        ..Default::default()
    })
    .await;

    let (db, _dir) = temp_db();
    let db2 = db.clone();
    let url = mock.url.clone();
    let handle = tokio::spawn(async move {
        samp_mirror::indexer::run_inner(&url, &db2, 42, 1).await
    });

    wait_for_block(&db, 3, 10).await;
    handle.abort();

    let lock = db.lock().await;
    let remarks = lock.remarks_by_type(0x10, 0);
    assert_eq!(remarks.len(), 1);
    assert_eq!(remarks[0].block, 3);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_run_inner_many_blocks_pipeline_increase() {
    let pubkey = test_pubkey();
    let remark = samp_remark(b"pipeline");

    let mut blocks = BTreeMap::new();
    for i in 1..=110 {
        blocks.insert(i, vec!["0x00".to_string()]);
    }
    blocks.insert(110, vec![build_samp_extrinsic(&pubkey, &remark)]);

    let mock = start_mock_node(MockNodeConfig {
        head: 110,
        blocks,
        ..Default::default()
    })
    .await;

    let (db, _dir) = temp_db();
    let db2 = db.clone();
    let url = mock.url.clone();
    let handle = tokio::spawn(async move {
        samp_mirror::indexer::run_inner(&url, &db2, 42, 1).await
    });

    wait_for_block(&db, 110, 30).await;
    handle.abort();

    let lock = db.lock().await;
    assert_eq!(lock.remarks_by_type(0x10, 0).len(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_run_inner_log_at_block_1000() {
    let pubkey = test_pubkey();
    let remark = samp_remark(b"1k");

    let mut blocks = BTreeMap::new();
    for i in 999..=1001 {
        blocks.insert(i, vec!["0x00".to_string()]);
    }
    blocks.insert(1001, vec![build_samp_extrinsic(&pubkey, &remark)]);

    let mock = start_mock_node(MockNodeConfig {
        head: 1001,
        blocks,
        ..Default::default()
    })
    .await;

    let (db, _dir) = temp_db();
    let db2 = db.clone();
    let url = mock.url.clone();
    let handle = tokio::spawn(async move {
        samp_mirror::indexer::run_inner(&url, &db2, 42, 999).await
    });

    wait_for_block(&db, 1001, 10).await;
    handle.abort();

    let lock = db.lock().await;
    assert_eq!(lock.remarks_by_type(0x10, 0).len(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_run_reconnect_loop() {
    let pubkey = test_pubkey();
    let remark = samp_remark(b"reconnect");

    let mut blocks = BTreeMap::new();
    blocks.insert(1, vec![build_samp_extrinsic(&pubkey, &remark)]);

    let mock = start_mock_node(MockNodeConfig {
        head: 1,
        blocks,
        ..Default::default()
    })
    .await;

    let (db, _dir) = temp_db();
    let db2 = db.clone();
    let url = mock.url.clone();
    let handle = tokio::spawn(async move {
        samp_mirror::indexer::run(url, db2, 42, 1).await
    });

    wait_for_block(&db, 1, 10).await;
    handle.abort();

    let lock = db.lock().await;
    assert_eq!(lock.remarks_by_type(0x10, 0).len(), 1);
}

#[tokio::test]
async fn test_serve_full_lifecycle() {
    let mock = start_mock_node(MockNodeConfig {
        head: 0,
        ..Default::default()
    })
    .await;

    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("serve.db").to_str().unwrap().to_string();
    let url = mock.url.clone();
    let handle = tokio::spawn(async move { samp_mirror::serve(url, &db_path, 0, 1).await });

    tokio::time::sleep(Duration::from_millis(500)).await;
    handle.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_serve_with_existing_data() {
    let mock = start_mock_node(MockNodeConfig {
        head: 0,
        ..Default::default()
    })
    .await;

    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("serve.db");

    {
        let db = Db::open(db_path.to_str().unwrap());
        db.insert_remark(&InsertRemark {
            block_number: 50,
            ext_index: 0,
            sender: "5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY",
            content_type: 0x10,
            channel_block: None,
            channel_index: None,
        });
    }

    let path = db_path.to_str().unwrap().to_string();
    let url = mock.url.clone();
    let handle = tokio::spawn(async move { samp_mirror::serve(url, &path, 0, 0).await });

    tokio::time::sleep(Duration::from_millis(500)).await;
    handle.abort();
}

#[tokio::test]
async fn test_serve_bad_node() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);

    let dir = tempfile::tempdir().unwrap();
    let result = samp_mirror::serve(
        format!("ws://127.0.0.1:{port}"),
        dir.path().join("test.db").to_str().unwrap(),
        0,
        1,
    )
    .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_run_inner_websocket_close_with_ping() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let url = format!("ws://127.0.0.1:{port}");

    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let ws = tokio_tungstenite::accept_async(stream).await.unwrap();
        let (mut write, mut read) = ws.split();

        while let Some(Ok(msg)) = read.next().await {
            let text = match msg {
                WsMessage::Text(t) => t.to_string(),
                _ => continue,
            };
            let req: Value = match serde_json::from_str(&text) {
                Ok(v) => v,
                Err(_) => continue,
            };
            let id = req["id"].clone();
            let method = req["method"].as_str().unwrap_or("");

            match method {
                "chain_getHeader" => {
                    let resp = json!({"jsonrpc":"2.0","id":id,"result":{"number":"0x0"}});
                    let _ =
                        write.send(WsMessage::Text(resp.to_string().into())).await;
                }
                "chain_subscribeNewHeads" => {
                    let resp = json!({"jsonrpc":"2.0","id":id,"result":"sub_1"});
                    let _ =
                        write.send(WsMessage::Text(resp.to_string().into())).await;
                }
                "chain_getBlock" => {
                    let resp = json!({"jsonrpc":"2.0","id":id,"result":null});
                    let _ =
                        write.send(WsMessage::Text(resp.to_string().into())).await;
                    let _ = write.send(WsMessage::Ping(vec![].into())).await;
                    break;
                }
                _ => {}
            }
        }
    });

    let (db, _dir) = temp_db();
    let result = samp_mirror::indexer::run_inner(&url, &db, 42, 1).await;
    assert!(result.is_err());
}

