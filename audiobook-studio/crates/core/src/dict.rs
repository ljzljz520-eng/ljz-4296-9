//! 词典：条目 / 版本 / 同形词例外与读音解析。

use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::clock::now_ts;
use crate::error::{Error, Result};
use crate::models::*;
use crate::Studio;

const ENTRY_COLS: &str =
    "id, headword, scope, kind, tags, current_version_id, created_at, updated_at";
const VERSION_COLS: &str = "id, entry_id, version_no, ipa, syllabification, example_audio, \
     source, status, note, created_at, approved_at";
const EXC_COLS: &str =
    "id, entry_id, scope_kind, scope_ref, ipa, syllabification, note, created_at";

fn map_entry(r: &Row) -> rusqlite::Result<Entry> {
    Ok(Entry {
        id: r.get(0)?,
        headword: r.get(1)?,
        scope: r.get(2)?,
        kind: r.get(3)?,
        tags: r.get(4)?,
        current_version_id: r.get(5)?,
        created_at: r.get(6)?,
        updated_at: r.get(7)?,
    })
}

fn map_version(r: &Row) -> rusqlite::Result<EntryVersion> {
    Ok(EntryVersion {
        id: r.get(0)?,
        entry_id: r.get(1)?,
        version_no: r.get(2)?,
        ipa: r.get(3)?,
        syllabification: r.get(4)?,
        example_audio: r.get(5)?,
        source: r.get(6)?,
        status: r.get(7)?,
        note: r.get(8)?,
        created_at: r.get(9)?,
        approved_at: r.get(10)?,
    })
}

fn map_exception(r: &Row) -> rusqlite::Result<EntryException> {
    Ok(EntryException {
        id: r.get(0)?,
        entry_id: r.get(1)?,
        scope_kind: r.get(2)?,
        scope_ref: r.get(3)?,
        ipa: r.get(4)?,
        syllabification: r.get(5)?,
        note: r.get(6)?,
        created_at: r.get(7)?,
    })
}

impl Studio {
    pub fn project_info(&self) -> Result<ProjectInfo> {
        Ok(ProjectInfo {
            name: self.project_name()?,
            revision: self.revision()?,
            created_at: self.meta("created_at")?.unwrap_or_default(),
            updated_at: self.meta("updated_at")?.unwrap_or_default(),
        })
    }

    // ---- 条目 ----------------------------------------------------------

    pub fn list_entries(&self) -> Result<Vec<Entry>> {
        let mut s = self.conn.prepare(&format!("SELECT {ENTRY_COLS} FROM entry ORDER BY headword"))?;
        let rows = s.query_map([], map_entry)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn get_entry(&self, id: &str) -> Result<Entry> {
        self.conn
            .query_row(&format!("SELECT {ENTRY_COLS} FROM entry WHERE id=?1"), [id], map_entry)
            .optional()?
            .ok_or_else(|| Error::NotFound(format!("词条 {id}")))
    }

    fn entry_by_headword(conn: &Connection, headword: &str) -> Result<Option<Entry>> {
        Ok(conn
            .query_row(&format!("SELECT {ENTRY_COLS} FROM entry WHERE headword=?1"), [headword], map_entry)
            .optional()?)
    }

    /// 新建词条并写入 v1。同名条目不允许重复——同形异读一律走“例外”。
    pub fn create_entry(&self, input: NewEntry) -> Result<(Entry, EntryVersion)> {
        if input.headword.trim().is_empty() {
            return Err(Error::Format("词条原文不能为空".into()));
        }
        if Self::entry_by_headword(&self.conn, &input.headword)?.is_some() {
            return Err(Error::Conflict(format!("已存在同名词条「{}」，请改用同形词例外", input.headword)));
        }
        let id = uuid::Uuid::new_v4().to_string();
        let vid = uuid::Uuid::new_v4().to_string();
        let ts = now_ts();
        let status = if input.approved { "approved" } else { "proposed" };
        let tx = self.conn.unchecked_transaction()?;
        tx.execute_batch("PRAGMA defer_foreign_keys=ON;")?;
        tx.execute(
            "INSERT INTO entry(id, headword, scope, kind, tags, current_version_id, created_at, updated_at)
             VALUES(?1,?2,?3,?4,?5,NULL,?6,?6)",
            params![id, input.headword, input.scope, input.kind, input.tags, ts],
        )?;
        tx.execute(
            "INSERT INTO entry_version(id, entry_id, version_no, ipa, syllabification, example_audio,
                                      source, status, note, created_at, approved_at)
             VALUES(?1,?2,1,?3,?4,?5,?6,?7,?8,?9,?10)",
            params![
                vid, id, input.ipa, input.syllabification, input.example_audio,
                input.source, status, input.note, ts,
                if input.approved { Some(&ts) } else { None }
            ],
        )?;
        if input.approved {
            tx.execute("UPDATE entry SET current_version_id=?1 WHERE id=?2", params![vid, id])?;
        }
        tx.commit()?;
        self.bump_revision()?;
        Ok((self.get_entry(&id)?, self.get_version(&vid)?))
    }

    // ---- 版本 ----------------------------------------------------------

    /// 全部版本（前端批注上展示锁定版本号用）。
    pub fn list_all_versions(&self) -> Result<Vec<EntryVersion>> {
        self.all_versions()
    }

    pub fn list_versions(&self, entry_id: &str) -> Result<Vec<EntryVersion>> {
        let mut s = self.conn.prepare(&format!(
            "SELECT {VERSION_COLS} FROM entry_version WHERE entry_id=?1 ORDER BY version_no"
        ))?;
        let rows = s.query_map([entry_id], map_version)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn get_version(&self, id: &str) -> Result<EntryVersion> {
        self.conn
            .query_row(&format!("SELECT {VERSION_COLS} FROM entry_version WHERE id=?1"), [id], map_version)
            .optional()?
            .ok_or_else(|| Error::NotFound(format!("词典版本 {id}")))
    }

    /// 为词条追加新版本。
    ///
    /// 只有 **批准的新版本** 才会切换 current_version，并对使用旧读音的录音片段
    /// 生成“待复核范围”；旧录音不会被直接判定为错误。
    /// 返回 (新版本, 生成的待复核条数)。
    pub fn add_version(&self, entry_id: &str, input: NewVersion) -> Result<(EntryVersion, usize)> {
        let entry = self.get_entry(entry_id)?;
        let next_no: i64 = self
            .conn
            .query_row("SELECT COALESCE(MAX(version_no),0)+1 FROM entry_version WHERE entry_id=?1", [entry_id], |r| r.get(0))?;
        let vid = uuid::Uuid::new_v4().to_string();
        let ts = now_ts();
        let status = if input.approved { "approved" } else { "proposed" };
        self.conn.execute(
            "INSERT INTO entry_version(id, entry_id, version_no, ipa, syllabification, example_audio,
                                      source, status, note, created_at, approved_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
            params![
                vid, entry_id, next_no, input.ipa, input.syllabification, input.example_audio,
                input.source, status, input.note, ts,
                if input.approved { Some(&ts) } else { None }
            ],
        )?;

        let mut ranges = 0;
        if input.approved {
            let old = entry.current_version_id.as_ref().map(|id| self.get_version(id)).transpose()?;
            if let Some(old) = old.as_ref() {
                if old.ipa != input.ipa || old.syllabification != input.syllabification {
                    ranges = self.generate_for_default_change(
                        entry_id, old, &input.ipa, &input.syllabification, &vid,
                    )?;
                }
            }
            self.conn.execute(
                "UPDATE entry SET current_version_id=?1, updated_at=?2 WHERE id=?3",
                params![vid, ts, entry_id],
            )?;
        }
        self.bump_revision()?;
        Ok((self.get_version(&vid)?, ranges))
    }

    /// 批准一个之前“待定”的版本；若读音变化同样生成待复核范围。
    pub fn approve_version(&self, version_id: &str) -> Result<usize> {
        let v = self.get_version(version_id)?;
        if v.status == "approved" {
            return Ok(0);
        }
        let entry = self.get_entry(&v.entry_id)?;
        let ts = now_ts();
        let old = entry.current_version_id.as_ref().map(|id| self.get_version(id)).transpose()?;
        let mut ranges = 0;
        if let Some(old) = old.as_ref() {
            if old.ipa != v.ipa || old.syllabification != v.syllabification {
                ranges = self.generate_for_default_change(
                    &v.entry_id, old, &v.ipa, &v.syllabification, version_id,
                )?;
            }
        }
        self.conn.execute(
            "UPDATE entry_version SET status='approved', approved_at=?1 WHERE id=?2",
            params![ts, version_id],
        )?;
        self.conn.execute(
            "UPDATE entry SET current_version_id=?1, updated_at=?2 WHERE id=?3",
            params![version_id, ts, v.entry_id],
        )?;
        self.bump_revision()?;
        Ok(ranges)
    }

    pub fn reject_version(&self, version_id: &str) -> Result<()> {
        let v = self.get_version(version_id)?;
        if v.status == "approved" {
            return Err(Error::Conflict("不能否决已批准版本，请改用新版本覆盖".into()));
        }
        self.conn.execute("UPDATE entry_version SET status='rejected' WHERE id=?1", [version_id])?;
        self.bump_revision()?;
        Ok(())
    }

    // ---- 同形词例外 ----------------------------------------------------

    pub fn list_exceptions(&self, entry_id: &str) -> Result<Vec<EntryException>> {
        let mut s = self.conn.prepare(&format!(
            "SELECT {EXC_COLS} FROM entry_exception WHERE entry_id=?1 ORDER BY scope_kind, scope_ref"
        ))?;
        let rows = s.query_map([entry_id], map_exception)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// 新建或更新某 (角色/章节) 的例外读音。例外读音变化同样生成待复核范围。
    pub fn upsert_exception(&self, entry_id: &str, input: NewException) -> Result<(EntryException, usize)> {
        self.get_entry(entry_id)?;
        let kind = match input.scope_kind.as_str() {
            "role" | "chapter" => input.scope_kind.as_str(),
            other => return Err(Error::Format(format!("例外作用类型非法: {other}"))),
        };
        if input.scope_ref.trim().is_empty() {
            return Err(Error::Format("例外作用对象（角色名/章节号）不能为空".into()));
        }
        let ts = now_ts();
        let existing: Option<EntryException> = self
            .conn
            .query_row(
                &format!("SELECT {EXC_COLS} FROM entry_exception WHERE entry_id=?1 AND scope_kind=?2 AND scope_ref=?3"),
                params![entry_id, kind, input.scope_ref],
                map_exception,
            )
            .optional()?;

        let mut ranges = 0;
        let exc = if let Some(old) = existing {
            if old.ipa != input.ipa || old.syllabification != input.syllabification {
                ranges = self.generate_for_exception_change(
                    entry_id, &old, &input.ipa, &input.syllabification,
                )?;
            }
            self.conn.execute(
                "UPDATE entry_exception SET ipa=?1, syllabification=?2, note=?3 WHERE id=?4",
                params![input.ipa, input.syllabification, input.note, old.id],
            )?;
            self.conn
                .query_row(&format!("SELECT {EXC_COLS} FROM entry_exception WHERE id=?1"), [&old.id], map_exception)?
        } else {
            let id = uuid::Uuid::new_v4().to_string();
            self.conn.execute(
                "INSERT INTO entry_exception(id, entry_id, scope_kind, scope_ref, ipa, syllabification, note, created_at)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
                params![id, entry_id, kind, input.scope_ref, input.ipa, input.syllabification, input.note, ts],
            )?;
            self.conn
                .query_row(&format!("SELECT {EXC_COLS} FROM entry_exception WHERE id=?1"), [&id], map_exception)?
        };
        self.bump_revision()?;
        Ok((exc, ranges))
    }

    pub fn delete_exception(&self, exception_id: &str) -> Result<()> {
        let n = self.conn.execute("DELETE FROM entry_exception WHERE id=?1", [exception_id])?;
        if n == 0 {
            return Err(Error::NotFound(format!("例外 {exception_id}")));
        }
        self.bump_revision()?;
        Ok(())
    }

    // ---- 解析 ----------------------------------------------------------

    /// 解析某原文在给定章节/角色语境下的有效读音。
    ///
    /// 优先级：角色例外 > 章节例外 > 词条当前批准版本。
    pub fn resolve(&self, q: &ResolveQuery) -> Result<ResolvedPron> {
        let entry = Self::entry_by_headword(&self.conn, &q.headword)?
            .ok_or_else(|| Error::NotFound(format!("词条「{}」", q.headword)))?;
        let vid = entry
            .current_version_id
            .clone()
            .ok_or_else(|| Error::NoApprovedVersion(entry.headword.clone()))?;
        let default_v = self.get_version(&vid)?;

        if let Some(role) = q.role.as_ref().filter(|s| !s.is_empty()) {
            if let Some(exc) = self.find_exception(&entry.id, "role", role)? {
                return Ok(ResolvedPron {
                    entry_id: entry.id,
                    headword: entry.headword,
                    version_id: default_v.id,
                    ipa: exc.ipa,
                    syllabification: exc.syllabification,
                    matched: "exception:role".into(),
                    exception_id: Some(exc.id),
                });
            }
        }
        if let Some(ch) = q.chapter {
            let key = ch.to_string();
            if let Some(exc) = self.find_exception(&entry.id, "chapter", &key)? {
                return Ok(ResolvedPron {
                    entry_id: entry.id,
                    headword: entry.headword,
                    version_id: default_v.id,
                    ipa: exc.ipa,
                    syllabification: exc.syllabification,
                    matched: "exception:chapter".into(),
                    exception_id: Some(exc.id),
                });
            }
        }
        Ok(ResolvedPron {
            entry_id: entry.id,
            headword: entry.headword,
            version_id: default_v.id,
            ipa: default_v.ipa,
            syllabification: default_v.syllabification,
            matched: "default".into(),
            exception_id: None,
        })
    }

    fn find_exception(&self, entry_id: &str, kind: &str, scope_ref: &str) -> Result<Option<EntryException>> {
        Ok(self
            .conn
            .query_row(
                &format!("SELECT {EXC_COLS} FROM entry_exception WHERE entry_id=?1 AND scope_kind=?2 AND scope_ref=?3"),
                params![entry_id, kind, scope_ref],
                map_exception,
            )
            .optional()?)
    }
}
