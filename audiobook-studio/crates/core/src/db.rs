//! SQLite 工程存储：打开/新建、schema 迁移、修订号。

use std::path::{Path, PathBuf};

use rusqlite::Connection;

use crate::clock::now_ts;
use crate::error::{Error, Result};

pub const SCHEMA_VERSION: i64 = 1;
/// 离线包 snapshot.json 的格式版本。
pub const PACKAGE_FORMAT: i64 = 1;

pub struct Studio {
    pub conn: Connection,
    /// 工程根目录（`studio.db` 所在目录）。
    pub root: PathBuf,
}

impl Studio {
    /// 打开工程；`dir` 不存在或目录里没有 studio.db 时新建。
    pub fn open(dir: impl AsRef<Path>) -> Result<Self> {
        let dir = dir.as_ref();
        std::fs::create_dir_all(dir.join("media"))?;
        let db_path = dir.join("studio.db");
        let fresh = !db_path.exists();
        let conn = Connection::open(&db_path)?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        // entry.current_version_id 与 entry_version.entry_id 构成循环外键，
        // 事务提交时再检查（先插词条 v1 再回填 current_version_id）。
        conn.pragma_update(None, "defer_foreign_keys", "ON")?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        let user_version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if user_version == 0 {
            conn.execute_batch(SCHEMA)?;
            conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        } else if user_version != SCHEMA_VERSION {
            return Err(Error::Format(format!(
                "工程 schema 版本 {user_version} 与本程序支持的 {SCHEMA_VERSION} 不一致"
            )));
        }
        if fresh {
            conn.execute(
                "INSERT INTO meta(key, value) VALUES ('project_name', ?1), ('revision', 1),
                 ('created_at', ?2), ('updated_at', ?2)",
                rusqlite::params![dir.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "新工程".into()), now_ts()],
            )?;
        }
        Ok(Studio { conn, root: dir.to_path_buf() })
    }

    /// 测试 / 内存工程。
    pub fn in_memory() -> Result<Self> {
        let dir = std::env::temp_dir().join(format!("abstudio-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("media"))?;
        let conn = Connection::open(dir.join("studio.db"))?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.pragma_update(None, "defer_foreign_keys", "ON")?;
        conn.execute_batch(SCHEMA)?;
        conn.execute(
            "INSERT INTO meta(key, value) VALUES ('project_name', '内存工程'), ('revision', 1),
             ('created_at', ?1), ('updated_at', ?1)",
            [now_ts()],
        )?;
        Ok(Studio { conn, root: dir })
    }

    pub fn project_name(&self) -> Result<String> {
        Ok(self.conn.query_row("SELECT value FROM meta WHERE key='project_name'", [], |r| r.get::<_, String>(0))?)
    }

    pub fn set_project_name(&self, name: &str) -> Result<()> {
        self.conn.execute("UPDATE meta SET value=?1 WHERE key='project_name'", [name])?;
        self.bump_revision()?;
        Ok(())
    }

    pub fn revision(&self) -> Result<i64> {
        Ok(self.conn.query_row("SELECT CAST(value AS INTEGER) FROM meta WHERE key='revision'", [], |r| r.get(0))?)
    }

    /// 修订号 +1 并刷新 updated_at。结构性业务变更都应调用。
    pub fn bump_revision(&self) -> Result<()> {
        self.conn.execute_batch(&format!(
            "UPDATE meta SET value = CAST(value AS INTEGER)+1 WHERE key='revision';
             UPDATE meta SET value='{}' WHERE key='updated_at';",
            now_ts()
        ))?;
        Ok(())
    }

    pub fn meta(&self, key: &str) -> Result<Option<String>> {
        let mut s = self.conn.prepare("SELECT value FROM meta WHERE key=?1")?;
        let mut rows = s.query([key])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row.get(0)?))
        } else {
            Ok(None)
        }
    }
}

const SCHEMA: &str = r#"
CREATE TABLE meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

-- 词典 ----------------------------------------------------------------
CREATE TABLE entry (
    id                 TEXT PRIMARY KEY,
    headword           TEXT NOT NULL,
    scope              TEXT NOT NULL DEFAULT '全书',
    kind               TEXT NOT NULL DEFAULT 'term',
    tags               TEXT NOT NULL DEFAULT '',
    current_version_id TEXT REFERENCES entry_version(id) DEFERRABLE INITIALLY DEFERRED,
    created_at         TEXT NOT NULL,
    updated_at         TEXT NOT NULL
);
CREATE INDEX idx_entry_headword ON entry(headword);

CREATE TABLE entry_version (
    id            TEXT PRIMARY KEY,
    entry_id      TEXT NOT NULL REFERENCES entry(id) ON DELETE CASCADE,
    version_no    INTEGER NOT NULL,
    ipa           TEXT NOT NULL DEFAULT '',
    syllabification TEXT NOT NULL DEFAULT '',
    example_audio TEXT,
    source        TEXT NOT NULL DEFAULT '',
    status        TEXT NOT NULL DEFAULT 'proposed',  -- proposed / approved / rejected
    note          TEXT NOT NULL DEFAULT '',
    created_at    TEXT NOT NULL,
    approved_at   TEXT,
    UNIQUE(entry_id, version_no)
);

CREATE TABLE entry_exception (
    id             TEXT PRIMARY KEY,
    entry_id       TEXT NOT NULL REFERENCES entry(id) ON DELETE CASCADE,
    scope_kind     TEXT NOT NULL CHECK(scope_kind IN ('role','chapter')),
    scope_ref      TEXT NOT NULL,
    ipa            TEXT NOT NULL DEFAULT '',
    syllabification TEXT NOT NULL DEFAULT '',
    note           TEXT NOT NULL DEFAULT '',
    created_at     TEXT NOT NULL,
    UNIQUE(entry_id, scope_kind, scope_ref)
);

-- 录音 ----------------------------------------------------------------
CREATE TABLE chapter (
    id         TEXT PRIMARY KEY,
    chapter_no INTEGER NOT NULL UNIQUE,
    title      TEXT NOT NULL DEFAULT ''
);

CREATE TABLE recording (
    id          TEXT PRIMARY KEY,
    chapter_id  TEXT NOT NULL REFERENCES chapter(id),
    item_no     TEXT NOT NULL,
    file_path   TEXT NOT NULL,
    rel_path    TEXT,
    sha256      TEXT NOT NULL,
    size_bytes  INTEGER NOT NULL,
    duration    REAL NOT NULL,
    peaks_json  TEXT NOT NULL DEFAULT '[]',
    imported_at TEXT NOT NULL,
    mtime       INTEGER NOT NULL,
    UNIQUE(chapter_id, item_no)
);

CREATE TABLE recording_occurrence (
    id           TEXT PRIMARY KEY,
    recording_id TEXT NOT NULL REFERENCES recording(id) ON DELETE CASCADE,
    entry_id     TEXT NOT NULL REFERENCES entry(id),
    role_ref     TEXT,
    time_start   REAL NOT NULL,
    time_end     REAL NOT NULL,
    CHECK(time_start >= 0 AND time_end > time_start)
);
CREATE INDEX idx_occ_rec ON recording_occurrence(recording_id);
CREATE INDEX idx_occ_entry ON recording_occurrence(entry_id);

-- 批注（不可变，只能 resolve） ------------------------------------------
CREATE TABLE annotation (
    id               TEXT PRIMARY KEY,
    recording_id     TEXT NOT NULL REFERENCES recording(id) ON DELETE CASCADE,
    kind             TEXT NOT NULL CHECK(kind IN ('mispron','stress','noise','other')),
    time_start       REAL NOT NULL,
    time_end         REAL NOT NULL,
    comment          TEXT NOT NULL DEFAULT '',
    entry_version_id TEXT REFERENCES entry_version(id),
    expected_ipa     TEXT,
    reviewer         TEXT NOT NULL DEFAULT '',
    created_at       TEXT NOT NULL,
    resolved         INTEGER NOT NULL DEFAULT 0,
    CHECK(time_start >= 0 AND time_end > time_start)
);
CREATE INDEX idx_ann_rec ON annotation(recording_id);
CREATE INDEX idx_ann_reviewer ON annotation(reviewer);

-- 待复核范围（修改读音后生成，而非直接判错旧录音） -------------------------
CREATE TABLE review_range (
    id              TEXT PRIMARY KEY,
    recording_id    TEXT NOT NULL REFERENCES recording(id) ON DELETE CASCADE,
    entry_id        TEXT REFERENCES entry(id),
    time_start      REAL NOT NULL,
    time_end        REAL NOT NULL,
    reason          TEXT NOT NULL,
    detail          TEXT NOT NULL DEFAULT '',
    from_version_id TEXT REFERENCES entry_version(id),
    to_version_id   TEXT REFERENCES entry_version(id),
    status          TEXT NOT NULL DEFAULT 'pending', -- pending / ok / reannotated / rerecord
    created_at      TEXT NOT NULL,
    resolved_by     TEXT,
    resolution      TEXT,
    CHECK(time_start >= 0 AND time_end > time_start)
);
CREATE INDEX idx_review_status ON review_range(status);
-- 去重键：同一录音、同一条目、同一时间段、同一原因只保留一条待复核
CREATE UNIQUE INDEX idx_review_dedup
    ON review_range(recording_id, COALESCE(entry_id, ''), time_start, time_end, reason);

-- 离线包导入台账 ---------------------------------------------------------
CREATE TABLE imported_package (
    id           TEXT PRIMARY KEY,
    applied_at   TEXT NOT NULL,
    source       TEXT NOT NULL DEFAULT '',
    revision     INTEGER,
    package_name TEXT NOT NULL DEFAULT ''
);
"#;
