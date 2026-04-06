use rusqlite::{params, Connection};

pub struct Db {
    conn: Connection,
}

pub struct InsertRemark<'a> {
    pub block_number: u32,
    pub ext_index: u16,
    pub sender: &'a str,
    pub timestamp_ms: u64,
    pub content_type: u8,
    pub remark_hex: &'a str,
    pub recipient: Option<&'a str>,
    pub channel_block: Option<u32>,
    pub channel_index: Option<u16>,
}

#[derive(serde::Serialize)]
pub struct RemarkRow {
    pub block: u32,
    pub index: u16,
    pub sender: String,
    pub timestamp: u64,
    pub remark: String,
}

#[derive(serde::Serialize)]
pub struct ChannelRow {
    pub block: u32,
    pub index: u16,
    pub creator: String,
    pub name: String,
    pub description: String,
    pub timestamp: u64,
}

impl Db {
    pub fn open(path: &str) -> Self {
        let conn = Connection::open(path).expect("open database");
        conn.execute_batch(
            "
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            PRAGMA busy_timeout = 5000;
        ",
        )
        .expect("set pragmas");
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS remarks (
                block_number   INTEGER NOT NULL,
                ext_index      INTEGER NOT NULL,
                sender         TEXT NOT NULL,
                timestamp_ms   INTEGER NOT NULL,
                content_type   INTEGER NOT NULL,
                remark_hex     TEXT NOT NULL,
                recipient      TEXT,
                channel_block  INTEGER,
                channel_index  INTEGER,
                PRIMARY KEY (block_number, ext_index)
            );
            CREATE TABLE IF NOT EXISTS sync_state (
                id         INTEGER PRIMARY KEY CHECK (id = 1),
                last_block INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_content_type ON remarks(content_type);
            CREATE INDEX IF NOT EXISTS idx_channel ON remarks(channel_block, channel_index);
            CREATE INDEX IF NOT EXISTS idx_sender ON remarks(sender);
            CREATE INDEX IF NOT EXISTS idx_block ON remarks(block_number);
        ",
        )
        .expect("create tables");
        Db { conn }
    }

    pub fn last_block(&self) -> u64 {
        self.conn
            .query_row(
                "SELECT last_block FROM sync_state WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0)
    }

    pub fn set_last_block(&self, block: u64) {
        self.conn
            .execute(
                "INSERT INTO sync_state (id, last_block) VALUES (1, ?1)
             ON CONFLICT(id) DO UPDATE SET last_block = ?1",
                params![block],
            )
            .expect("update sync state");
    }

    pub fn insert_remark(&self, r: &InsertRemark) {
        let _ = self.conn.execute(
            "INSERT OR IGNORE INTO remarks
             (block_number, ext_index, sender, timestamp_ms, content_type, remark_hex,
              recipient, channel_block, channel_index)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                r.block_number,
                r.ext_index,
                r.sender,
                r.timestamp_ms,
                r.content_type,
                r.remark_hex,
                r.recipient,
                r.channel_block,
                r.channel_index,
            ],
        );
    }

    pub fn channels(&self) -> Vec<ChannelRow> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT block_number, ext_index, sender, timestamp_ms, remark_hex
             FROM remarks WHERE content_type = 19 ORDER BY block_number, ext_index",
            )
            .unwrap();
        stmt.query_map([], |row| {
            let remark_hex: String = row.get(4)?;
            let remark_bytes = hex::decode(&remark_hex).unwrap_or_default();
            let (name, description) = if remark_bytes.len() > 1 {
                samp::decode_channel_create(&remark_bytes[1..])
                    .map(|(n, d)| (n.to_string(), d.to_string()))
                    .unwrap_or_default()
            } else {
                (String::new(), String::new())
            };
            Ok(ChannelRow {
                block: row.get(0)?,
                index: row.get::<_, u32>(1)? as u16,
                creator: row.get(2)?,
                name,
                description,
                timestamp: row.get::<_, u64>(3)? / 1000,
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
    }

    pub fn channel_messages(&self, ch_block: u32, ch_index: u16, after: u64) -> Vec<RemarkRow> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT block_number, ext_index, sender, timestamp_ms, remark_hex
             FROM remarks WHERE content_type = 20 AND channel_block = ?1 AND channel_index = ?2
             AND block_number > ?3 ORDER BY block_number, ext_index",
            )
            .unwrap();
        stmt.query_map(params![ch_block, ch_index, after], |row| {
            Ok(RemarkRow {
                block: row.get(0)?,
                index: row.get::<_, u32>(1)? as u16,
                sender: row.get(2)?,
                timestamp: row.get::<_, u64>(3)? / 1000,
                remark: row.get(4)?,
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
    }

    pub fn remarks_by_type(&self, content_type: u8, after: u64) -> Vec<RemarkRow> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT block_number, ext_index, sender, timestamp_ms, remark_hex
             FROM remarks WHERE content_type = ?1 AND block_number > ?2
             ORDER BY block_number, ext_index",
            )
            .unwrap();
        stmt.query_map(params![content_type, after], |row| {
            Ok(RemarkRow {
                block: row.get(0)?,
                index: row.get::<_, u32>(1)? as u16,
                sender: row.get(2)?,
                timestamp: row.get::<_, u64>(3)? / 1000,
                remark: row.get(4)?,
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
    }

    pub fn remarks_by_sender(&self, sender: &str, after: u64) -> Vec<RemarkRow> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT block_number, ext_index, sender, timestamp_ms, remark_hex
             FROM remarks WHERE sender = ?1 AND block_number > ?2
             ORDER BY block_number, ext_index",
            )
            .unwrap();
        stmt.query_map(params![sender, after], |row| {
            Ok(RemarkRow {
                block: row.get(0)?,
                index: row.get::<_, u32>(1)? as u16,
                sender: row.get(2)?,
                timestamp: row.get::<_, u64>(3)? / 1000,
                remark: row.get(4)?,
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
    }
}
