use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_tungstenite::{connect_async, tungstenite::Message as WsMessage};

use crate::db::Db;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RemarkCallIds {
    pub remark: Option<(u8, u8)>,
    pub remark_with_event: (u8, u8),
}

impl RemarkCallIds {
    pub fn from_metadata(metadata: &samp::metadata::Metadata) -> Result<Self, String> {
        let remark = metadata.find_call_index("System", "remark");
        let remark_with_event = metadata
            .find_call_index("System", "remark_with_event")
            .ok_or("runtime does not expose System.remark_with_event")?;
        Ok(Self {
            remark,
            remark_with_event,
        })
    }

    pub fn accepts(self, pair: (u8, u8)) -> bool {
        self.remark == Some(pair) || self.remark_with_event == pair
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChainInfo {
    pub chain: String,
    pub ss58_prefix: u16,
    pub remark_calls: RemarkCallIds,
}

/// Fetch chain name, SS58 prefix, and remark call IDs from the node via RPC.
pub async fn fetch_chain_info(node_url: &str) -> Result<ChainInfo, String> {
    let (ws, _) = connect_async(node_url)
        .await
        .map_err(|e| format!("connect: {e}"))?;
    let (mut write, mut read) = ws.split();

    let req = json!({ "jsonrpc": "2.0", "id": 1, "method": "system_chain", "params": [] });
    write
        .send(WsMessage::Text(req.to_string().into()))
        .await
        .map_err(|e| format!("send: {e}"))?;
    let resp = next_text(&mut read).await?;
    let v: Value = serde_json::from_str(&resp).map_err(|e| format!("parse: {e}"))?;
    let chain = v["result"]
        .as_str()
        .ok_or("missing system_chain result")?
        .to_string();

    let req = json!({ "jsonrpc": "2.0", "id": 2, "method": "system_properties", "params": [] });
    write
        .send(WsMessage::Text(req.to_string().into()))
        .await
        .map_err(|e| format!("send: {e}"))?;
    let resp = next_text(&mut read).await?;
    let v: Value = serde_json::from_str(&resp).map_err(|e| format!("parse: {e}"))?;
    let ss58_raw = v["result"]["ss58Format"]
        .as_u64()
        .ok_or("missing ss58Format")?;
    let ss58_prefix =
        u16::try_from(ss58_raw).map_err(|_| format!("ss58Format out of range: {ss58_raw}"))?;
    samp::Ss58Prefix::new(ss58_prefix).map_err(|e| format!("ss58Format unsupported: {e}"))?;

    let req = json!({ "jsonrpc": "2.0", "id": 3, "method": "state_getMetadata", "params": [] });
    write
        .send(WsMessage::Text(req.to_string().into()))
        .await
        .map_err(|e| format!("send: {e}"))?;
    let resp = next_text(&mut read).await?;
    let v: Value = serde_json::from_str(&resp).map_err(|e| format!("parse: {e}"))?;
    if let Some(err) = v.get("error") {
        return Err(format!("metadata RPC error: {err}"));
    }
    let metadata_hex = v["result"]
        .as_str()
        .ok_or("missing state_getMetadata result")?;
    let metadata_bytes =
        hex::decode(metadata_hex.trim_start_matches("0x")).map_err(|e| format!("hex: {e}"))?;
    let metadata = samp::metadata::Metadata::from_runtime_metadata(&metadata_bytes)
        .map_err(|e| format!("metadata: {e}"))?;
    let remark_calls = RemarkCallIds::from_metadata(&metadata)?;

    Ok(ChainInfo {
        chain,
        ss58_prefix,
        remark_calls,
    })
}

pub async fn run(
    node_url: String,
    db: Arc<Mutex<Db>>,
    ss58_prefix: u16,
    remark_calls: RemarkCallIds,
    start_block: u64,
) {
    loop {
        if let Err(e) = run_inner(&node_url, &db, ss58_prefix, remark_calls, start_block).await {
            tracing::error!("Indexer error: {e}. Reconnecting in 5s...");
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        }
    }
}

enum BlockState {
    WaitingForHash { block_num: u64 },
    WaitingForBlock { block_num: u64 },
}

pub async fn run_inner(
    node_url: &str,
    db: &Arc<Mutex<Db>>,
    ss58_prefix: u16,
    remark_calls: RemarkCallIds,
    start_block: u64,
) -> Result<(), String> {
    let (ws, _) = connect_async(node_url)
        .await
        .map_err(|e| format!("connect: {e}"))?;
    let (mut write, mut read) = ws.split();
    tracing::info!("Connected to {node_url}");

    let req = json!({ "jsonrpc": "2.0", "id": 1, "method": "chain_getHeader", "params": [] });
    write
        .send(WsMessage::Text(req.to_string().into()))
        .await
        .map_err(|e| format!("send: {e}"))?;
    let resp = next_text(&mut read).await?;
    let v: Value = serde_json::from_str(&resp).map_err(|e| format!("parse: {e}"))?;
    let head = u64::from_str_radix(
        v["result"]["number"]
            .as_str()
            .ok_or("no block number")?
            .trim_start_matches("0x"),
        16,
    )
    .map_err(|e| format!("parse: {e}"))?;

    let last_block = db.lock().await.last_block();
    let resume_from = if last_block > 0 {
        last_block + 1
    } else {
        start_block.max(1)
    };

    if resume_from <= head {
        tracing::info!(
            "Catching up: {resume_from} -> {head} ({} blocks)",
            head - resume_from + 1
        );
        catch_up(
            &mut write,
            &mut read,
            db,
            resume_from,
            head,
            ss58_prefix,
            remark_calls,
        )
        .await?;
        tracing::info!("Catch-up complete at block {head}");
    }

    let sub_msg = json!({
        "jsonrpc": "2.0", "id": 1,
        "method": "chain_subscribeNewHeads", "params": []
    });
    write
        .send(WsMessage::Text(sub_msg.to_string().into()))
        .await
        .map_err(|e| format!("subscribe: {e}"))?;

    let mut request_id: u64 = 1_000_000;

    while let Some(Ok(msg)) = read.next().await {
        let text = match &msg {
            WsMessage::Text(t) => t.to_string(),
            WsMessage::Ping(_) | WsMessage::Pong(_) => continue,
            _ => continue,
        };
        let v: Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(_) => continue,
        };

        if let Some(number_hex) = v["params"]["result"]["number"].as_str() {
            let block_num =
                u64::from_str_radix(number_hex.trim_start_matches("0x"), 16).unwrap_or(0);
            request_id += 1;
            let req = json!({ "jsonrpc": "2.0", "id": request_id, "method": "chain_getBlockHash", "params": [block_num] });
            write
                .send(WsMessage::Text(req.to_string().into()))
                .await
                .map_err(|e| format!("send: {e}"))?;
            continue;
        }

        if let Some(result) = v.get("result") {
            if let Some(hash) = result.as_str() {
                request_id += 1;
                let req = json!({ "jsonrpc": "2.0", "id": request_id, "method": "chain_getBlock", "params": [hash] });
                write
                    .send(WsMessage::Text(req.to_string().into()))
                    .await
                    .map_err(|e| format!("send: {e}"))?;
            } else if let Some(block) = result.get("block") {
                let block_num = block["header"]["number"]
                    .as_str()
                    .and_then(|s| u64::from_str_radix(s.trim_start_matches("0x"), 16).ok())
                    .unwrap_or(0);
                process_block(block, block_num, db, ss58_prefix, remark_calls).await;
            }
        }
    }

    Err("WebSocket closed".into())
}

/// Async pipelined catch-up: sends multiple requests ahead, processes responses as they arrive.
async fn catch_up(
    write: &mut futures_util::stream::SplitSink<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        WsMessage,
    >,
    read: &mut futures_util::stream::SplitStream<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
    >,
    db: &Arc<Mutex<Db>>,
    start: u64,
    end: u64,
    ss58_prefix: u16,
    remark_calls: RemarkCallIds,
) -> Result<(), String> {
    let max_depth: usize = 20;
    let mut pipeline_depth: usize = 10;
    let mut consecutive_ok: u64 = 0;

    let mut in_flight: BTreeMap<u64, BlockState> = BTreeMap::new();
    let mut ready_blocks: BTreeMap<u64, Value> = BTreeMap::new();

    let mut next_to_send: u64 = start;
    let mut next_to_process: u64 = start;
    let mut request_id: u64 = 100;

    loop {
        while in_flight.len() < pipeline_depth * 2 && next_to_send <= end {
            request_id += 1;
            let req = json!({ "jsonrpc": "2.0", "id": request_id, "method": "chain_getBlockHash", "params": [next_to_send] });
            write
                .send(WsMessage::Text(req.to_string().into()))
                .await
                .map_err(|e| format!("send: {e}"))?;
            in_flight.insert(
                request_id,
                BlockState::WaitingForHash {
                    block_num: next_to_send,
                },
            );
            next_to_send += 1;
        }

        let text = next_text(read).await?;
        let v: Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let resp_id = v["id"].as_u64().unwrap_or(0);

        if v.get("error").is_some() {
            tracing::warn!("RPC error for request {resp_id}: {}", v["error"]);
            pipeline_depth = (pipeline_depth / 2).max(1);
            consecutive_ok = 0;
            if let Some(state) = in_flight.remove(&resp_id) {
                let block_num = match state {
                    BlockState::WaitingForHash { block_num }
                    | BlockState::WaitingForBlock { block_num } => block_num,
                };
                if block_num < next_to_send {
                    next_to_send = block_num;
                }
            }
            continue;
        }

        if let Some(state) = in_flight.remove(&resp_id) {
            match state {
                BlockState::WaitingForHash { block_num } => {
                    if let Some(hash) = v["result"].as_str() {
                        request_id += 1;
                        let req = json!({ "jsonrpc": "2.0", "id": request_id, "method": "chain_getBlock", "params": [hash] });
                        write
                            .send(WsMessage::Text(req.to_string().into()))
                            .await
                            .map_err(|e| format!("send: {e}"))?;
                        in_flight.insert(request_id, BlockState::WaitingForBlock { block_num });
                    }
                }
                BlockState::WaitingForBlock { block_num } => {
                    if let Some(block) = v["result"].get("block") {
                        ready_blocks.insert(block_num, block.clone());
                        consecutive_ok += 1;

                        if consecutive_ok.is_multiple_of(100) && pipeline_depth < max_depth {
                            pipeline_depth = (pipeline_depth + 2).min(max_depth);
                        }
                    }
                }
            }
        }

        while let Some(block_data) = ready_blocks.remove(&next_to_process) {
            process_block(&block_data, next_to_process, db, ss58_prefix, remark_calls).await;
            if next_to_process.is_multiple_of(1000) {
                tracing::info!("Synced to block {next_to_process} (pipeline: {pipeline_depth})");
            }
            next_to_process += 1;
        }

        if next_to_process > end {
            break;
        }
    }

    Ok(())
}

pub async fn process_block(
    block: &Value,
    block_num: u64,
    db: &Arc<Mutex<Db>>,
    ss58_prefix: u16,
    remark_calls: RemarkCallIds,
) {
    let extrinsics = match block["extrinsics"].as_array() {
        Some(exts) => exts,
        None => return,
    };

    let block_number_typed = match samp::BlockNumber::try_from_u64(block_num) {
        Ok(n) => n,
        Err(_) => {
            tracing::error!("Block {block_num} exceeds u32::MAX, skipping");
            return;
        }
    };
    let block_number_u32 = block_number_typed.get();

    let mut count = 0u32;

    let prefix_typed = match samp::Ss58Prefix::new(ss58_prefix) {
        Ok(p) => p,
        Err(_) => return,
    };

    for (ext_index, ext) in extrinsics.iter().enumerate() {
        let ext_hex = match ext.as_str() {
            Some(s) => s,
            None => continue,
        };
        let raw = match hex::decode(ext_hex.trim_start_matches("0x")) {
            Ok(b) => b,
            Err(_) => continue,
        };
        let ext_bytes = samp::ExtrinsicBytes::from_bytes(raw);

        let sender = match samp::extract_signer(&ext_bytes) {
            Some(s) => s,
            None => continue,
        };
        let call = match samp::extract_call(&ext_bytes) {
            Some(c) => c,
            None => continue,
        };
        let call_id = (call.pallet().get(), call.call().get());
        if !remark_calls.accepts(call_id) {
            continue;
        }
        let remark = match samp::scale::decode_bytes(call.args().as_bytes()) {
            Some((r, _)) => r,
            None => continue,
        };

        if !samp::is_samp_remark(remark) {
            continue;
        }

        let content_type = remark[0];
        let sender_ss58 = sender.to_ss58(prefix_typed).as_str().to_string();

        let mut channel_block: Option<u32> = None;
        let mut channel_index: Option<u16> = None;
        if content_type & 0x0F == 0x04 && remark.len() >= 7 {
            channel_block = Some(u32::from_le_bytes(remark[1..5].try_into().unwrap()));
            channel_index = Some(u16::from_le_bytes(remark[5..7].try_into().unwrap()));
        }

        let ext_index_u16 = match samp::ExtIndex::try_from_usize(ext_index) {
            Ok(i) => i.get(),
            Err(_) => continue,
        };
        let db = db.lock().await;
        db.insert_remark(&crate::db::InsertRemark {
            block_number: block_number_u32,
            ext_index: ext_index_u16,
            sender: &sender_ss58,
            content_type,
            channel_block,
            channel_index,
        });
        if content_type & 0x0F == 0x03 {
            db.insert_channel(block_number_u32, ext_index_u16);
        }
        count += 1;
    }

    if count > 0 {
        tracing::info!("Block {block_num}: indexed {count} SAMP remark(s)");
    }
}

async fn next_text(
    read: &mut futures_util::stream::SplitStream<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
    >,
) -> Result<String, String> {
    loop {
        match read.next().await {
            Some(Ok(WsMessage::Text(t))) => return Ok(t.to_string()),
            Some(Ok(WsMessage::Ping(_) | WsMessage::Pong(_))) => continue,
            Some(Ok(other)) => return Err(format!("unexpected: {other:?}")),
            Some(Err(e)) => return Err(format!("ws: {e}")),
            None => return Err("closed".into()),
        }
    }
}
