//! 审听：时间点批注与“待复核范围”。
//!
//! 修改读音不会自动判旧录音错误——只生成 pending 的 review_range，
//! 由复核者决定 ok / reannotated / rerecord。

use rusqlite::params;

use crate::clock::now_ts;
use crate::error::{Error, Result};
use crate::models::*;
use crate::Studio;

/// 允许 1ms 浮点误差。
const EPS: f64 = 0.001;

const ANN_COLS: &str = "id, recording_id, kind, time_start, time_end, comment, \
     entry_version_id, expected_ipa, reviewer, created_at, resolved";
const RANGE_COLS: &str = "id, recording_id, entry_id, time_start, time_end, reason, detail, \
     from_version_id, to_version_id, status, created_at, resolved_by, resolution";

fn map_annotation(r: &rusqlite::Row) -> rusqlite::Result<Annotation> {
    Ok(Annotation {
        id: r.get(0)?,
        recording_id: r.get(1)?,
        kind: r.get(2)?,
        time_start: r.get(3)?,
        time_end: r.get(4)?,
        comment: r.get(5)?,
        entry_version_id: r.get(6)?,
        expected_ipa: r.get(7)?,
        reviewer: r.get(8)?,
        created_at: r.get(9)?,
        resolved: r.get(10)?,
    })
}

fn map_range(r: &rusqlite::Row) -> rusqlite::Result<ReviewRange> {
    Ok(ReviewRange {
        id: r.get(0)?,
        recording_id: r.get(1)?,
        entry_id: r.get(2)?,
        time_start: r.get(3)?,
        time_end: r.get(4)?,
        reason: r.get(5)?,
        detail: r.get(6)?,
        from_version_id: r.get(7)?,
        to_version_id: r.get(8)?,
        status: r.get(9)?,
        created_at: r.get(10)?,
        resolved_by: r.get(11)?,
        resolution: r.get(12)?,
    })
}

/// 通用时间段合法性（不依赖录音时长）。
pub fn validate_window(start: f64, end: f64) -> Result<()> {
    if !start.is_finite() || !end.is_finite() || start < 0.0 || end <= start {
        return Err(Error::BadRange(start, end));
    }
    Ok(())
}

impl Studio {
    // ---- 批注 ----------------------------------------------------------

    pub fn list_annotations(&self, recording_id: &str) -> Result<Vec<Annotation>> {
        let mut s = self.conn.prepare(&format!(
            "SELECT {ANN_COLS} FROM annotation WHERE recording_id=?1 ORDER BY time_start"
        ))?;
        let rows = s.query_map([recording_id], map_annotation)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn list_all_annotations(&self) -> Result<Vec<Annotation>> {
        let mut s = self
            .conn
            .prepare(&format!("SELECT {ANN_COLS} FROM annotation ORDER BY recording_id, time_start"))?;
        let rows = s.query_map([], map_annotation)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// 添加批注。越界（超出片段时长、负时间、start>=end）直接拒绝。
    pub fn add_annotation(&self, input: NewAnnotation) -> Result<Annotation> {
        let reviewer = input.reviewer.filter(|s| !s.trim().is_empty()).unwrap_or_else(|| "匿名".into());
        let id = self.insert_annotation(
            &uuid::Uuid::new_v4().to_string(),
            input.recording_id.as_str(),
            input.kind.as_str(),
            input.time_start,
            input.time_end,
            &input.comment,
            input.entry_version_id.as_deref(),
            None,
            &reviewer,
            &now_ts(),
            true, // 越界拒绝
        )?;
        self.bump_revision()?;
        self.get_annotation(&id)
    }

    /// 供离线包合并使用：保留原 id / 时间戳，返回冲突说明而不是直接报错。
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn insert_annotation(
        &self,
        id: &str,
        recording_id: &str,
        kind: &str,
        start: f64,
        end: f64,
        comment: &str,
        version_id: Option<&str>,
        expected_ipa: Option<&str>,
        reviewer: &str,
        created_at: &str,
        strict_bounds: bool,
    ) -> Result<String> {
        validate_window(start, end)?;
        let duration: f64 = self
            .conn
            .query_row("SELECT duration FROM recording WHERE id=?1", [recording_id], |r| r.get(0))
            .map_err(|_| Error::NotFound(format!("录音片段 {recording_id}")))?;
        if strict_bounds && end > duration + EPS {
            return Err(Error::OutOfBounds(id.to_string(), duration, start, end));
        }
        let expected = match (expected_ipa, version_id) {
            (Some(e), _) => Some(e.to_string()),
            (None, Some(vid)) => Some(self.get_version(vid)?.ipa),
            _ => None,
        };
        if let Some(vid) = version_id {
            // 指向的词典版本必须存在（即便词条被删，外键也会兜底）。
            self.get_version(vid)?;
        }
        self.conn.execute(
            "INSERT INTO annotation(id, recording_id, kind, time_start, time_end, comment,
                                    entry_version_id, expected_ipa, reviewer, created_at, resolved)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,0)",
            params![id, recording_id, kind, start, end, comment, version_id, expected, reviewer, created_at],
        )?;
        Ok(id.to_string())
    }

    pub fn get_annotation(&self, id: &str) -> Result<Annotation> {
        self.conn
            .query_row(&format!("SELECT {ANN_COLS} FROM annotation WHERE id=?1"), [id], map_annotation)
            .map_err(|_| Error::NotFound(format!("批注 {id}")))
    }

    pub fn resolve_annotation(&self, id: &str) -> Result<()> {
        let n = self.conn.execute("UPDATE annotation SET resolved=1 WHERE id=?1", [id])?;
        if n == 0 {
            return Err(Error::NotFound(format!("批注 {id}")));
        }
        self.bump_revision()?;
        Ok(())
    }

    // ---- 待复核范围 ----------------------------------------------------

    /// 默认读音变化：为“确实使用默认读音”的出现位置生成待复核。
    /// 被角色/章节例外覆盖的出现不受默认读音变化影响。
    pub(crate) fn generate_for_default_change(
        &self,
        entry_id: &str,
        old: &EntryVersion,
        new_ipa: &str,
        new_syl: &str,
        to_version_id: &str,
    ) -> Result<usize> {
        let detail = format!("默认读音 {} [{} {}] → [{} {}]", old.version_no, old.ipa, old.syllabification, new_ipa, new_syl);
        // 该词条的每个出现，排除：
        //  1) occurrence.role_ref 命中角色例外
        //  2) 录音所在章节命中章节例外
        let mut sel = self.conn.prepare(
            "SELECT o.id, o.recording_id, o.time_start, o.time_end
               FROM recording_occurrence o
               JOIN recording r ON r.id = o.recording_id
               JOIN chapter c ON c.id = r.chapter_id
              WHERE o.entry_id = ?1
                AND (o.role_ref IS NULL
                     OR NOT EXISTS (SELECT 1 FROM entry_exception e
                                     WHERE e.entry_id=?1 AND e.scope_kind='role'
                                       AND e.scope_ref=o.role_ref))
                AND NOT EXISTS (SELECT 1 FROM entry_exception e
                                 WHERE e.entry_id=?1 AND e.scope_kind='chapter'
                                   AND e.scope_ref=CAST(c.chapter_no AS TEXT))",
        )?;
        let rows: Vec<(String, String, f64, f64)> = sel
            .query_map([entry_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
            .collect::<rusqlite::Result<_>>()?;
        drop(sel);
        let mut n = 0;
        for (_oid, rec_id, s, e) in rows {
            n += self.upsert_review_range(&rec_id, Some(entry_id), s, e, "pron_changed", &detail, Some(&old.id), Some(to_version_id))?;
        }
        Ok(n)
    }

    /// 例外读音变化：仅覆盖命中该例外的出现。
    pub(crate) fn generate_for_exception_change(
        &self,
        entry_id: &str,
        old: &EntryException,
        new_ipa: &str,
        _new_syl: &str,
    ) -> Result<usize> {
        let detail = format!("{} 例外「{}」读音 [{}] → [{}]",
            if old.scope_kind == "role" { "角色" } else { "章节" }, old.scope_ref, old.ipa, new_ipa);
        let sql = if old.scope_kind == "role" {
            "SELECT o.recording_id, o.time_start, o.time_end
               FROM recording_occurrence o WHERE o.entry_id=?1 AND o.role_ref=?2"
        } else {
            "SELECT o.recording_id, o.time_start, o.time_end
               FROM recording_occurrence o
               JOIN recording r ON r.id=o.recording_id
               JOIN chapter c ON c.id=r.chapter_id
              WHERE o.entry_id=?1 AND CAST(c.chapter_no AS TEXT)=?2"
        };
        let mut sel = self.conn.prepare(sql)?;
        let rows: Vec<(String, f64, f64)> = sel
            .query_map(params![entry_id, old.scope_ref], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<rusqlite::Result<_>>()?;
        drop(sel);
        let mut n = 0;
        for (rec_id, s, e) in rows {
            n += self.upsert_review_range(&rec_id, Some(entry_id), s, e, "pron_changed", &detail, None, None)?;
        }
        Ok(n)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn upsert_review_range(
        &self,
        recording_id: &str,
        entry_id: Option<&str>,
        start: f64,
        end: f64,
        reason: &str,
        detail: &str,
        from_v: Option<&str>,
        to_v: Option<&str>,
    ) -> Result<usize> {
        let id = uuid::Uuid::new_v4().to_string();
        let ts = now_ts();
        self.conn.execute(
            "INSERT INTO review_range(id, recording_id, entry_id, time_start, time_end, reason, detail,
                                      from_version_id, to_version_id, status, created_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,'pending',?10)
             ON CONFLICT(recording_id, COALESCE(entry_id, ''), time_start, time_end, reason)
             DO UPDATE SET status='pending', detail=excluded.detail,
                           from_version_id=excluded.from_version_id,
                           to_version_id=excluded.to_version_id,
                           resolved_by=NULL, resolution=NULL, created_at=excluded.created_at
             WHERE review_range.status != 'pending'",
            params![id, recording_id, entry_id, start, end, reason, detail, from_v, to_v, ts],
        )?;
        Ok(self.conn.changes() as usize)
    }

    pub fn list_pending_reviews(&self) -> Result<Vec<ReviewRange>> {
        let mut s = self.conn.prepare(&format!(
            "SELECT {RANGE_COLS} FROM review_range WHERE status='pending' ORDER BY created_at"
        ))?;
        let rows = s.query_map([], map_range)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn list_reviews_for_recording(&self, recording_id: &str) -> Result<Vec<ReviewRange>> {
        let mut s = self.conn.prepare(&format!(
            "SELECT {RANGE_COLS} FROM review_range WHERE recording_id=?1 ORDER BY time_start"
        ))?;
        let rows = s.query_map([recording_id], map_range)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// 复核结论：ok（旧录音可接受）/ reannotated（已转批注）/ rerecord（需重录）。
    pub fn resolve_review(&self, id: &str, resolution: &str, reviewer: &str) -> Result<()> {
        if !matches!(resolution, "ok" | "reannotated" | "rerecord") {
            return Err(Error::Format(format!("复核结论非法: {resolution}")));
        }
        let n = self.conn.execute(
            "UPDATE review_range SET status=?1, resolved_by=?2, resolution=?1 WHERE id=?3 AND status='pending'",
            params![resolution, reviewer, id],
        )?;
        if n == 0 {
            return Err(Error::Conflict("待复核范围不存在或已处理".into()));
        }
        self.bump_revision()?;
        Ok(())
    }
}
