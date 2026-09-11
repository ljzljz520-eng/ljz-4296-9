//! 录音片段：导入（按章节+条次）、文件摘要、外部替换检测。

use std::path::{Path, PathBuf};

use rusqlite::params;
use sha2::{Digest, Sha256};

use crate::clock::now_ts;
use crate::error::{Error, Result};
use crate::models::*;
use crate::review::validate_window;
use crate::Studio;

const REC_COLS: &str = "id, chapter_id, item_no, file_path, rel_path, sha256, size_bytes, \
     duration, peaks_json, imported_at, mtime";
const OCC_COLS: &str = "id, recording_id, entry_id, role_ref, time_start, time_end";

fn map_recording(r: &rusqlite::Row) -> rusqlite::Result<Recording> {
    let peaks_json: String = r.get(8)?;
    Ok(Recording {
        id: r.get(0)?,
        chapter_id: r.get(1)?,
        item_no: r.get(2)?,
        file_path: r.get(3)?,
        rel_path: r.get(4)?,
        sha256: r.get(5)?,
        size_bytes: r.get(6)?,
        duration: r.get(7)?,
        peaks: serde_json::from_str(&peaks_json).unwrap_or_default(),
        imported_at: r.get(9)?,
        mtime: r.get(10)?,
    })
}

fn map_occurrence(r: &rusqlite::Row) -> rusqlite::Result<RecordingOccurrence> {
    Ok(RecordingOccurrence {
        id: r.get(0)?,
        recording_id: r.get(1)?,
        entry_id: r.get(2)?,
        role_ref: r.get(3)?,
        time_start: r.get(4)?,
        time_end: r.get(5)?,
    })
}

pub fn sha256_bytes(data: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(data);
    let d = h.finalize();
    let mut s = String::with_capacity(64);
    for b in d {
        use std::fmt::Write;
        let _ = write!(s, "{b:02x}");
    }
    s
}

impl Studio {
    // ---- 章节 ----------------------------------------------------------

    fn ensure_chapter(&self, no: i64, title: &str) -> Result<String> {
        if let Ok(id) = self
            .conn
            .query_row::<String, _, _>("SELECT id FROM chapter WHERE chapter_no=?1", [no], |r| r.get(0))
        {
            if !title.is_empty() {
                self.conn.execute("UPDATE chapter SET title=?1 WHERE id=?2", params![title, id])?;
            }
            return Ok(id);
        }
        let id = uuid::Uuid::new_v4().to_string();
        self.conn.execute(
            "INSERT INTO chapter(id, chapter_no, title) VALUES(?1,?2,?3)",
            params![id, no, title],
        )?;
        Ok(id)
    }

    pub fn list_chapters(&self) -> Result<Vec<Chapter>> {
        let mut s = self.conn.prepare("SELECT id, chapter_no, title FROM chapter ORDER BY chapter_no")?;
        let rows = s.query_map([], |r| {
            Ok(Chapter { id: r.get(0)?, chapter_no: r.get(1)?, title: r.get(2)? })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    // ---- 录音 ----------------------------------------------------------

    /// 导入一个录音文件：读取/校验时长与峰值、计算 sha256、复制进 media/、登记摘要。
    pub fn import_recording(&self, input: NewRecording) -> Result<Recording> {
        let src = PathBuf::from(&input.src_path);
        let bytes = std::fs::read(&src)?;
        let sha = sha256_bytes(&bytes);
        let mtime = std::fs::metadata(&src)?
            .modified()?
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        // 时长与峰值：WAV 在后端解析；其他格式信任前端 Web Audio 解码回传的时长。
        let (duration, peaks) = match crate::wav::parse(&bytes) {
            Some(info) => (info.duration, info.peaks),
            None => match input.duration {
                Some(d) if d.is_finite() && d > 0.0 => (d, vec![]),
                _ => return Err(Error::Format("非 WAV 文件需要前端提供解码时长".into())),
            },
        };

        let chapter_id = self.ensure_chapter(input.chapter_no, &input.chapter_title)?;
        let dup: Option<i64> = self
            .conn
            .query_row(
                "SELECT 1 FROM recording r JOIN chapter c ON c.id=r.chapter_id
                  WHERE c.chapter_no=?1 AND r.item_no=?2",
                params![input.chapter_no, input.item_no],
                |r| r.get(0),
            )
            .ok();
        if dup.is_some() {
            return Err(Error::Conflict(format!("第{}章条次「{}」已存在录音", input.chapter_no, input.item_no)));
        }

        let id = uuid::Uuid::new_v4().to_string();
        let (file_path, rel_path) = if input.copy_into_project {
            let ext = src.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
            let rel = format!("media/{}-{}{}", input.chapter_no, id, ext);
            std::fs::write(self.root.join(&rel), &bytes)?;
            (self.root.join(&rel).to_string_lossy().to_string(), Some(rel))
        } else {
            (input.src_path.clone(), None)
        };

        self.conn.execute(
            "INSERT INTO recording(id, chapter_id, item_no, file_path, rel_path, sha256, size_bytes,
                                   duration, peaks_json, imported_at, mtime)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
            params![
                id,
                chapter_id,
                input.item_no,
                file_path,
                rel_path,
                sha,
                bytes.len() as i64,
                duration,
                serde_json::to_string(&peaks)?,
                now_ts(),
                mtime
            ],
        )?;
        self.bump_revision()?;
        self.get_recording(&id)
    }

    pub fn list_recordings(&self) -> Result<Vec<Recording>> {
        let mut s = self.conn.prepare(&format!("SELECT {REC_COLS} FROM recording ORDER BY chapter_id, item_no"))?;
        let rows = s.query_map([], map_recording)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn get_recording(&self, id: &str) -> Result<Recording> {
        self.conn
            .query_row(&format!("SELECT {REC_COLS} FROM recording WHERE id=?1"), [id], map_recording)
            .map_err(|_| Error::NotFound(format!("录音 {id}")))
    }

    /// 检测文件是否被外部替换：重算 sha256 并与登记摘要对比。
    ///
    /// - 文件丢失：exists=false。
    /// - 内容一致：changed=false。
    /// - 被替换：更新 sha/大小/时长（若新文件时长不同），并列出落在新时长之外的批注；
    ///   这些批注不会被自动删除，交给审听者处理（可能需要重新定位或归档）。
    pub fn verify_recording(&self, id: &str) -> Result<VerifyResult> {
        let rec = self.get_recording(id)?;
        let path = Path::new(&rec.file_path);
        if !path.exists() {
            return Ok(VerifyResult {
                recording_id: id.into(),
                changed: false,
                old_sha: rec.sha256,
                new_sha: None,
                exists: false,
                old_duration: rec.duration,
                new_duration: None,
                oob_annotations: vec![],
                note: "文件不存在（可能被移动/重命名）".into(),
            });
        }
        let bytes = std::fs::read(path)?;
        let new_sha = sha256_bytes(&bytes);
        if new_sha == rec.sha256 {
            return Ok(VerifyResult {
                recording_id: id.into(),
                changed: false,
                old_sha: rec.sha256,
                new_sha: Some(new_sha),
                exists: true,
                old_duration: rec.duration,
                new_duration: Some(rec.duration),
                oob_annotations: vec![],
                note: "未变化".into(),
            });
        }

        // 文件被外部替换
        let new_info = crate::wav::parse(&bytes);
        let new_duration = new_info.as_ref().map(|i| i.duration);
        let mtime = std::fs::metadata(path)?
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        if let Some(info) = &new_info {
            self.conn.execute(
                "UPDATE recording SET sha256=?1, size_bytes=?2, duration=?3, peaks_json=?4, mtime=?5 WHERE id=?6",
                params![new_sha, bytes.len() as i64, info.duration, serde_json::to_string(&info.peaks)?, mtime, id],
            )?;
        } else {
            self.conn.execute(
                "UPDATE recording SET sha256=?1, size_bytes=?2, mtime=?3 WHERE id=?4",
                params![new_sha, bytes.len() as i64, mtime, id],
            )?;
        }

        // 越界批注（以新时长为准；非 WAV 无法解析时不更新时长，跳过此项检查）
        let mut oob = vec![];
        if let Some(d) = new_duration {
            for ann in self.list_annotations(id)? {
                if ann.time_end > d + 0.001 {
                    oob.push(ann);
                }
            }
        }
        self.bump_revision()?;
        Ok(VerifyResult {
            recording_id: id.into(),
            changed: true,
            old_sha: rec.sha256,
            new_sha: Some(new_sha),
            exists: true,
            old_duration: rec.duration,
            new_duration,
            oob_annotations: oob,
            note: "文件内容已被外部替换，摘要已更新".into(),
        })
    }

    // ---- 出现位置 ------------------------------------------------------

    /// 在录音片段上登记某词条的出现位置；读音按当时的章节/角色语境解析。
    pub fn add_occurrence(&self, input: NewOccurrence) -> Result<RecordingOccurrence> {
        validate_window(input.time_start, input.time_end)?;
        let rec = self.get_recording(&input.recording_id)?;
        if input.time_end > rec.duration + 0.001 {
            return Err(Error::OutOfBounds(
                input.recording_id,
                rec.duration,
                input.time_start,
                input.time_end,
            ));
        }
        let resolved = self.resolve(&ResolveQuery {
            headword: input.headword,
            chapter: input.chapter,
            role: input.role.clone(),
        })?;
        let id = uuid::Uuid::new_v4().to_string();
        self.conn.execute(
            "INSERT INTO recording_occurrence(id, recording_id, entry_id, role_ref, time_start, time_end)
             VALUES(?1,?2,?3,?4,?5,?6)",
            params![id, input.recording_id, resolved.entry_id, input.role, input.time_start, input.time_end],
        )?;
        self.bump_revision()?;
        Ok(self.conn
            .query_row(&format!("SELECT {OCC_COLS} FROM recording_occurrence WHERE id=?1"), [&id], map_occurrence)?)
    }

    pub fn list_occurrences(&self, recording_id: &str) -> Result<Vec<RecordingOccurrence>> {
        let mut s = self.conn.prepare(&format!(
            "SELECT {OCC_COLS} FROM recording_occurrence WHERE recording_id=?1 ORDER BY time_start"
        ))?;
        let rows = s.query_map([recording_id], map_occurrence)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn remove_occurrence(&self, id: &str) -> Result<()> {
        let n = self.conn.execute("DELETE FROM recording_occurrence WHERE id=?1", [id])?;
        if n == 0 {
            return Err(Error::NotFound(format!("出现位置 {id}")));
        }
        self.bump_revision()?;
        Ok(())
    }

    pub fn get_occurrence(&self, id: &str) -> Result<RecordingOccurrence> {
        self.conn
            .query_row(&format!("SELECT {OCC_COLS} FROM recording_occurrence WHERE id=?1"), [id], map_occurrence)
            .map_err(|_| Error::NotFound(format!("出现位置 {id}")))
    }
}
