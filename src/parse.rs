/// Decode a SCALE compact-encoded integer. Returns (value, bytes_consumed).
pub fn decode_compact(data: &[u8]) -> Option<(u64, usize)> {
    if data.is_empty() {
        return None;
    }
    match data[0] & 0b11 {
        0b00 => Some(((data[0] >> 2) as u64, 1)),
        0b01 => {
            if data.len() < 2 {
                return None;
            }
            Some(((u16::from_le_bytes([data[0], data[1]]) >> 2) as u64, 2))
        }
        0b10 => {
            if data.len() < 4 {
                return None;
            }
            Some((
                (u32::from_le_bytes([data[0], data[1], data[2], data[3]]) >> 2) as u64,
                4,
            ))
        }
        _ => {
            let bytes_following = ((data[0] >> 2) + 4) as usize;
            if data.len() < 1 + bytes_following {
                return None;
            }
            let mut buf = [0u8; 8];
            let n = bytes_following.min(8);
            buf[..n].copy_from_slice(&data[1..1 + n]);
            Some((u64::from_le_bytes(buf), 1 + bytes_following))
        }
    }
}

/// Extract the 32-byte account ID (signer) from a signed extrinsic.
pub fn extract_signer(ext_bytes: &[u8]) -> Option<[u8; 32]> {
    let (_, prefix_len) = decode_compact(ext_bytes)?;
    let payload = &ext_bytes[prefix_len..];
    // Signed extrinsic: bit 7 of version byte is set
    if payload.len() < 34 || payload[0] & 0x80 == 0 || payload[1] != 0x00 {
        return None;
    }
    let mut account = [0u8; 32];
    account.copy_from_slice(&payload[2..34]);
    Some(account)
}

/// Extract the remark payload from a `system.remark_with_event` extrinsic.
pub fn extract_remark(ext_bytes: &[u8]) -> Option<Vec<u8>> {
    let (_, prefix_len) = decode_compact(ext_bytes)?;
    let payload = &ext_bytes[prefix_len..];
    // Must be signed (bit 7 set)
    if payload.len() < 103 || payload[0] & 0x80 == 0 {
        return None;
    }
    // Skip header: version(1) + addr_type(1) + account(32) + sig_type(1) + sig(64) = 99
    let mut offset = 99;
    // Era
    if offset >= payload.len() {
        return None;
    }
    if payload[offset] != 0x00 {
        offset += 2;
    } else {
        offset += 1;
    }
    // Nonce (compact)
    let (_, nonce_len) = decode_compact(&payload[offset..])?;
    offset += nonce_len;
    // Tip (compact)
    let (_, tip_len) = decode_compact(&payload[offset..])?;
    offset += tip_len;
    // Metadata hash disabled byte
    offset += 1;
    // Call: pallet(1) + call(1)
    if offset + 2 >= payload.len() {
        return None;
    }
    let pallet = payload[offset];
    let call = payload[offset + 1];
    offset += 2;
    // system.remark_with_event = pallet 0x00, call 0x07 or 0x09
    if pallet != 0x00 || (call != 0x07 && call != 0x09) {
        return None;
    }
    // Remark length (compact) + data
    let (remark_len, compact_len) = decode_compact(&payload[offset..])?;
    offset += compact_len;
    let remark_len = remark_len as usize;
    if offset + remark_len > payload.len() {
        return None;
    }
    Some(payload[offset..offset + remark_len].to_vec())
}

/// Extract block timestamp (milliseconds) from the timestamp inherent.
/// The timestamp inherent is the first unsigned extrinsic in a Substrate block.
pub fn extract_block_timestamp(extrinsics: &[serde_json::Value]) -> u64 {
    for ext in extrinsics {
        let ext_hex = match ext.as_str() {
            Some(s) => s,
            None => continue,
        };
        let ext_bytes = match hex::decode(ext_hex.trim_start_matches("0x")) {
            Ok(b) => b,
            Err(_) => continue,
        };
        let (_, prefix_len) = match decode_compact(&ext_bytes) {
            Some(v) => v,
            None => continue,
        };
        let payload = &ext_bytes[prefix_len..];
        // Unsigned: bit 7 clear
        if payload.is_empty() || payload[0] & 0x80 != 0 {
            continue;
        }
        if payload.len() < 4 {
            continue;
        }
        // Skip version(1) + pallet(1) + call(1), read compact u64
        if let Some((ts_ms, _)) = decode_compact(&payload[3..]) {
            if ts_ms > 1_000_000_000_000 {
                return ts_ms;
            }
        }
    }
    0
}

/// Check if remark bytes are a SAMP remark.
pub fn is_samp(remark: &[u8]) -> bool {
    !remark.is_empty() && remark[0] & 0xF0 == samp::SAMP_VERSION
}

/// Convert a 32-byte account ID to SS58 address with the given prefix.
pub fn to_ss58(pubkey: &[u8; 32], prefix: u16) -> String {
    let mut body = Vec::new();
    if prefix < 64 {
        body.push(prefix as u8);
    } else {
        // Two-byte prefix encoding
        let first = ((prefix & 0b0000_0000_1111_1100) >> 2) | 0b01000000;
        let second = (prefix >> 8) | ((prefix & 0b11) << 6);
        body.push(first as u8);
        body.push(second as u8);
    }
    body.extend_from_slice(pubkey);

    use blake2::digest::{Update, VariableOutput};
    let mut hasher = blake2::Blake2bVar::new(64).unwrap();
    hasher.update(b"SS58PRE");
    hasher.update(&body);
    let mut hash = [0u8; 64];
    hasher.finalize_variable(&mut hash).unwrap();

    body.extend_from_slice(&hash[..2]);
    bs58::encode(body).into_string()
}
