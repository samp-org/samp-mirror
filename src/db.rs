use rusqlite::{params, Connection};

pub struct Db {
    conn: Connection,
}

pub struct InsertRemark<'a> {
    pub block_number: u32,
    pub ext_index: u16,
    pub sender: &'a str,
    pub content_type: u8,
    pub channel_block: Option<u32>,
    pub channel_index: Option<u16>,
}

#[derive(serde::Serialize)]
pub struct Hint {
    pub block: u32,
    pub index: u16,
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
                content_type   INTEGER NOT NULL,
                sender         TEXT NOT NULL,
                channel_block  INTEGER,
                channel_index  INTEGER,
                PRIMARY KEY (block_number, ext_index)
            );
            CREATE TABLE IF NOT EXISTS channels (
                block_number   INTEGER NOT NULL,
                ext_index      INTEGER NOT NULL,
                PRIMARY KEY (block_number, ext_index)
            );
            DROP TABLE IF EXISTS sync_state;
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
                "SELECT COALESCE(MAX(block_number), 0) FROM remarks",
                [],
                |row| row.get::<_, i64>(0).map(|v| v as u64),
            )
            .unwrap_or(0)
    }

    pub fn insert_remark(&self, r: &InsertRemark) {
        let _ = self.conn.execute(
            "INSERT OR IGNORE INTO remarks
             (block_number, ext_index, content_type, sender, channel_block, channel_index)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                r.block_number,
                r.ext_index,
                r.content_type,
                r.sender,
                r.channel_block,
                r.channel_index,
            ],
        );
    }

    pub fn insert_channel(&self, block_number: u32, ext_index: u16) {
        let _ = self.conn.execute(
            "INSERT OR IGNORE INTO channels (block_number, ext_index) VALUES (?1, ?2)",
            params![block_number, ext_index],
        );
    }

    pub fn channels(&self) -> Vec<Hint> {
        let mut stmt = self
            .conn
            .prepare("SELECT block_number, ext_index FROM channels ORDER BY block_number, ext_index")
            .unwrap();
        stmt.query_map([], |row| {
            Ok(Hint {
                block: row.get(0)?,
                index: row.get::<_, u32>(1)? as u16,
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
    }

    pub fn channel_messages(&self, ch_block: u32, ch_index: u16, after: u64) -> Vec<Hint> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT block_number, ext_index
             FROM remarks WHERE content_type = 20 AND channel_block = ?1 AND channel_index = ?2
             AND block_number > ?3 ORDER BY block_number, ext_index",
            )
            .unwrap();
        stmt.query_map(params![ch_block, ch_index, after], |row| {
            Ok(Hint {
                block: row.get(0)?,
                index: row.get::<_, u32>(1)? as u16,
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
    }

    pub fn remarks_by_type(&self, content_type: u8, after: u64) -> Vec<Hint> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT block_number, ext_index
             FROM remarks WHERE content_type = ?1 AND block_number > ?2
             ORDER BY block_number, ext_index",
            )
            .unwrap();
        stmt.query_map(params![content_type, after], |row| {
            Ok(Hint {
                block: row.get(0)?,
                index: row.get::<_, u32>(1)? as u16,
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
    }

    pub fn remarks_by_sender(&self, sender: &str, after: u64) -> Vec<Hint> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT block_number, ext_index
             FROM remarks WHERE sender = ?1 AND block_number > ?2
             ORDER BY block_number, ext_index",
            )
            .unwrap();
        stmt.query_map(params![sender, after], |row| {
            Ok(Hint {
                block: row.get(0)?,
                index: row.get::<_, u32>(1)? as u16,
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
    }
}
