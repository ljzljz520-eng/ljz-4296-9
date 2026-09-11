//! 离线包（.abpkg，本质是 zip）：导出快照 + 媒体，导入并多审听者合并。

use std::io::{Cursor, Read, Write};
use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use zip::{ZipArchive, ZipWriter};

use crate::clock::now_ts;
use crate::db::PACKAGE_FORMAT;
use crate::error::{Error, Result};
use crate::models::*;
use crate::review::validate_window;
use crate::Studio;

const SNAPSHOT_NAME: &str = "snapshot.json";
const MANIFEST_NAME: &str = "manifest.json";

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    format: i64,
    project: String,
    revision: i64,
    exported_at: String,
    exported_by: Option<String>,
    media: std::collections::BTreeMap<String, String>, // relPath -> sha256
}

// ---- 快照 --------------------------------------------------------------

impl Studio {
    /// 从当前工程生成快照。
    pub fn snapshot(&self, opts: &PackageOptions) -> Result<Snapshot> {
        let annotations = self.list_all_annotations()?;
        let annotations = match &opts.reviewer_filter {
            Some(name) if !name.is_empty() => annotations.into_iter().filter(|a| &a.reviewer == name).collect(),
            _ => annotations,
        };
        let mut snap = Snapshot {
            format: PACKAGE_FORMAT,
            project: self.project_name()?,
            revision: self.revision()?,
            exported_at: now_ts(),
            exported_by: None,
            source_path: Some(self.root.to_string_lossy().to_string()),
            entries: self.list_entries()?,
            versions: self.all_versions()?,
            exceptions: self.all_exceptions()?,
            chapters: self.list_chapters()?,
            recordings: self.list_recordings()?,
            occurrences: self.all_occurrences()?,
            annotations,
            review_ranges: self.all_review_ranges()?,
            media_count: 0,
        };
        if opts.include_media {
            snap.media_count = snap.recordings.iter().filter(|r| r.rel_path.is_some()).count();
        }
        Ok(snap)
    }

    pub(crate) fn all_versions(&self) -> Result<Vec<EntryVersion>> {
        let mut s = self.conn.prepare(
            "SELECT id, entry_id, version_no, ipa, syllabification, example_audio, source, status,
                    note, created_at, approved_at
             FROM entry_version ORDER BY entry_id, version_no",
        )?;
        let rows = s.query_map([], |r| {
            Ok(EntryVersion {
                id: r.get(0)?, entry_id: r.get(1)?, version_no: r.get(2)?, ipa: r.get(3)?,
                syllabification: r.get(4)?, example_audio: r.get(5)?, source: r.get(6)?,
                status: r.get(7)?, note: r.get(8)?, created_at: r.get(9)?, approved_at: r.get(10)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub(crate) fn all_exceptions(&self) -> Result<Vec<EntryException>> {
        let mut s = self.conn.prepare(
            "SELECT id, entry_id, scope_kind, scope_ref, ipa, syllabification, note, created_at
             FROM entry_exception",
        )?;
        let rows = s.query_map([], |r| {
            Ok(EntryException {
                id: r.get(0)?, entry_id: r.get(1)?, scope_kind: r.get(2)?, scope_ref: r.get(3)?,
                ipa: r.get(4)?, syllabification: r.get(5)?, note: r.get(6)?, created_at: r.get(7)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub(crate) fn all_occurrences(&self) -> Result<Vec<RecordingOccurrence>> {
        let mut s = self.conn.prepare(
            "SELECT id, recording_id, entry_id, role_ref, time_start, time_end
             FROM recording_occurrence ORDER BY recording_id, time_start",
        )?;
        let rows = s.query_map([], |r| {
            Ok(RecordingOccurrence {
                id: r.get(0)?, recording_id: r.get(1)?, entry_id: r.get(2)?,
                role_ref: r.get(3)?, time_start: r.get(4)?, time_end: r.get(5)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub(crate) fn all_review_ranges(&self) -> Result<Vec<ReviewRange>> {
        let mut s = self.conn.prepare(
            "SELECT id, recording_id, entry_id, time_start, time_end, reason, detail,
                    from_version_id, to_version_id, status, created_at, resolved_by, resolution
             FROM review_range ORDER BY created_at",
        )?;
        let rows = s.query_map([], |r| {
            Ok(ReviewRange {
                id: r.get(0)?, recording_id: r.get(1)?, entry_id: r.get(2)?, time_start: r.get(3)?,
                time_end: r.get(4)?, reason: r.get(5)?, detail: r.get(6)?,
                from_version_id: r.get(7)?, to_version_id: r.get(8)?, status: r.get(9)?,
                created_at: r.get(10)?, resolved_by: r.get(11)?, resolution: r.get(12)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    // ---- 导出 ----------------------------------------------------------

    /// 导出 .abpkg（zip）到 out_path。
    pub fn export_package(
        &self,
        out_path: impl AsRef<Path>,
        opts: &PackageOptions,
        exported_by: Option<&str>,
    ) -> Result<String> {
        let mut snap = self.snapshot(opts)?;
        snap.exported_by = exported_by.map(str::to_string);

        let mut media_manifest = std::collections::BTreeMap::new();
        let file = std::fs::File::create(out_path.as_ref())?;
        let mut zip = ZipWriter::new(file);
        let opts_zip = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

        // snapshot.json 先写；manifest.json 在收集完媒体清单后追加。

        zip.start_file(SNAPSHOT_NAME, opts_zip)?;
        zip.write_all(&serde_json::to_vec_pretty(&snap)?)?;

        if opts.include_media {
            for rec in &snap.recordings {
                if let Some(rel) = &rec.rel_path {
                    let p = self.root.join(rel);
                    if p.exists() {
                        let bytes = std::fs::read(&p)?;
                        zip.start_file(rel.clone(), opts_zip)?;
                        zip.write_all(&bytes)?;
                        media_manifest.insert(rel.clone(), rec.sha256.clone());
                    }
                } else if Path::new(&rec.file_path).exists() {
                    // 未复制进工程的外部文件：以 media/<recordingId> 存放
                    let bytes = std::fs::read(&rec.file_path)?;
                    let ext = Path::new(&rec.file_path)
                        .extension()
                        .map(|e| format!(".{}", e.to_string_lossy()))
                        .unwrap_or_default();
                    let key = format!("media/{}{ext}", rec.id);
                    zip.start_file(&key, opts_zip)?;
                    zip.write_all(&bytes)?;
                    media_manifest.insert(key, rec.sha256.clone());
                }
            }
        }
        snap.media_count = media_manifest.len();

        let manifest = Manifest {
            format: PACKAGE_FORMAT,
            project: snap.project.clone(),
            revision: snap.revision,
            exported_at: snap.exported_at.clone(),
            exported_by: snap.exported_by.clone(),
            media: media_manifest,
        };
        // 清单放最后追加（读取方按文件名读取，顺序无关）。
        zip.start_file(MANIFEST_NAME, opts_zip)?;
        zip.write_all(&serde_json::to_vec_pretty(&manifest)?)?;
        zip.finish()?;
        Ok(out_path.as_ref().to_string_lossy().to_string())
    }

    // ---- 导入 ----------------------------------------------------------

    /// 导入并合并离线包。整个合并是幂等的，可安全重复导入。
    pub fn import_package(&self, pkg_path: impl AsRef<Path>) -> Result<MergeReport> {
        let bytes = std::fs::read(pkg_path.as_ref())?;
        self.import_package_bytes(&bytes, Some(&pkg_path.as_ref().to_string_lossy()))
    }

    pub fn import_package_bytes(&self, bytes: &[u8], source: Option<&str>) -> Result<MergeReport> {
        let mut zip = ZipArchive::new(Cursor::new(bytes))
            .map_err(|e| Error::Package(format!("不是有效的 .abpkg: {e}")))?;

        let snap: Snapshot = {
            let mut f = zip
                .by_name(SNAPSHOT_NAME)
                .map_err(|e| Error::Package(format!("缺少 {SNAPSHOT_NAME}: {e}")))?;
            let mut s = String::new();
            f.read_to_string(&mut s)?;
            serde_json::from_str(&s)?
        };
        if snap.format != PACKAGE_FORMAT {
            return Err(Error::Package(format!(
                "离线包格式版本 {} 不受支持（需要 {PACKAGE_FORMAT}）",
                snap.format
            )));
        }
        let manifest: Option<Manifest> = zip.by_name(MANIFEST_NAME).ok().map(|mut f| {
            let mut s = String::new();
            f.read_to_string(&mut s)?;
            Ok::<_, Error>(serde_json::from_str(&s)?)
        }).transpose()?;

        let mut report = MergeReport::default();
        report.packages_applied.push(format!("{}@rev{}", snap.project, snap.revision));

        // 媒体先落盘（导入新录音需要）。
        let mut media_dir_files: Vec<(String, Vec<u8>)> = Vec::new();
        if let Some(m) = &manifest {
            for key in m.media.keys() {
                if let Ok(mut f) = zip.by_name(key) {
                    let mut buf = Vec::new();
                    f.read_to_end(&mut buf)?;
                    media_dir_files.push((key.clone(), buf));
                }
            }
        }

        let tx = self.conn.unchecked_transaction()?;
        // 词条/版本互为外键，推迟到事务提交时检查。
        tx.execute_batch("PRAGMA defer_foreign_keys=ON;")?;

        import_chapters(&tx, &snap)?;
        import_entries(&tx, &snap, &mut report)?;
        import_recordings(&tx, &snap, &mut report, self)?;
        import_occurrences(&tx, &snap, &mut report)?;
        import_annotations(&tx, &snap, &mut report)?;
        import_review_ranges(&tx, &snap, &mut report)?;

        tx.execute(
            "INSERT INTO imported_package(id, applied_at, source, revision, package_name)
             VALUES(?1,?2,?3,?4,?5)",
            params![
                uuid::Uuid::new_v4().to_string(),
                now_ts(),
                source.unwrap_or("memory"),
                snap.revision,
                snap.project
            ],
        )?;
        tx.commit()?;

        // 媒体写入在事务外（IO），但因按 sha 命名/登记，重复导入也安全。
        for (key, data) in media_dir_files {
            let p = self.root.join(&key);
            if let Some(parent) = p.parent() {
                std::fs::create_dir_all(parent)?;
            }
            if !p.exists() {
                std::fs::write(p, data)?;
            }
        }

        self.bump_revision()?;
        Ok(report)
    }
}

// ---- 合并原语（幂等） ----------------------------------------------------

fn id_exists(conn: &Connection, table: &str, id: &str) -> Result<bool> {
    let sql = format!("SELECT 1 FROM {table} WHERE id=?1");
    Ok(conn.query_row(&sql, [id], |_| Ok(())).optional()?.is_some())
}

fn import_chapters(conn: &Connection, snap: &Snapshot) -> Result<()> {
    for ch in &snap.chapters {
        let exists: Option<i64> = conn
            .query_row("SELECT 1 FROM chapter WHERE chapter_no=?1", [ch.chapter_no], |r| r.get(0))
            .ok();
        if exists.is_none() {
            conn.execute(
                "INSERT INTO chapter(id, chapter_no, title) VALUES(?1,?2,?3)",
                params![ch.id, ch.chapter_no, ch.title],
            )?;
        }
    }
    Ok(())
}

fn import_entries(conn: &Connection, snap: &Snapshot, report: &mut MergeReport) -> Result<()> {
    for e in &snap.entries {
        if !id_exists(conn, "entry", &e.id)? {
            conn.execute(
                "INSERT INTO entry(id, headword, scope, kind, tags, current_version_id, created_at, updated_at)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
                params![
                    e.id, e.headword, e.scope, e.kind, e.tags, e.current_version_id,
                    e.created_at, e.updated_at
                ],
            )?;
        }
    }
    for v in &snap.versions {
        if !id_exists(conn, "entry_version", &v.id)? {
            conn.execute(
                "INSERT INTO entry_version(id, entry_id, version_no, ipa, syllabification, example_audio,
                                           source, status, note, created_at, approved_at)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
                params![
                    v.id, v.entry_id, v.version_no, v.ipa, v.syllabification, v.example_audio,
                    v.source, v.status, v.note, v.created_at, v.approved_at
                ],
            )?;
            report.versions_added += 1;
        }
    }
    for x in &snap.exceptions {
        let same_key: Option<String> = conn
            .query_row(
                "SELECT id FROM entry_exception WHERE entry_id=?1 AND scope_kind=?2 AND scope_ref=?3",
                params![x.entry_id, x.scope_kind, x.scope_ref],
                |r| r.get(0),
            )
            .optional()?;
        match same_key {
            Some(local_id) if local_id == x.id => {}
            Some(_) => {
                // 同 (条目, 角色/章节) 已有例外且内容不同：不覆盖，记冲突。
                let local_ipa: String = conn.query_row(
                    "SELECT ipa FROM entry_exception WHERE entry_id=?1 AND scope_kind=?2 AND scope_ref=?3",
                    params![x.entry_id, x.scope_kind, x.scope_ref],
                    |r| r.get(0),
                )?;
                if local_ipa != x.ipa {
                    report.conflicts.push(MergeConflict {
                        kind: "exception_divergent".into(),
                        id: x.id.clone(),
                        detail: format!("{}「{}」例外读音不一致：本地 [{}] / 包内 [{}]",
                            x.scope_kind, x.scope_ref, local_ipa, x.ipa),
                    });
                }
            }
            None => {
                conn.execute(
                    "INSERT INTO entry_exception(id, entry_id, scope_kind, scope_ref, ipa, syllabification,
                                                note, created_at)
                     VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
                    params![
                        x.id, x.entry_id, x.scope_kind, x.scope_ref, x.ipa, x.syllabification,
                        x.note, x.created_at
                    ],
                )?;
                report.exceptions_added += 1;
            }
        }
    }
    Ok(())
}

fn import_recordings(conn: &Connection, snap: &Snapshot, report: &mut MergeReport, studio: &Studio) -> Result<()> {
    for r in &snap.recordings {
        if id_exists(conn, "recording", &r.id)? {
            let local_sha: String =
                conn.query_row("SELECT sha256 FROM recording WHERE id=?1", [&r.id], |row| row.get(0))?;
            if local_sha != r.sha256 {
                report.conflicts.push(MergeConflict {
                    kind: "recording_divergent".into(),
                    id: r.id.clone(),
                    detail: format!("同 ID 录音内容不同（本地 {:.8}… / 包内 {:.8}…）", local_sha, r.sha256),
                });
            }
            continue;
        }
        // 新录音：章节必须已存在（import_chapters 已处理同号章节）。
        let chapter_id: String = match conn
            .query_row("SELECT id FROM chapter WHERE id=?1", [&r.chapter_id], |row| row.get(0))
            .optional()?
        {
            Some(id) => id,
            None => {
                report.skipped.push(format!("录音 {} 缺少章节，已跳过", r.item_no));
                continue;
            }
        };
        // 媒体复制进工程时，路径改写到本地工程。
        let (file_path, rel_path) = match &r.rel_path {
            Some(rel) => (studio.root.join(rel).to_string_lossy().to_string(), Some(rel.clone())),
            None => (r.file_path.clone(), None),
        };
        conn.execute(
            "INSERT INTO recording(id, chapter_id, item_no, file_path, rel_path, sha256, size_bytes,
                                   duration, peaks_json, imported_at, mtime)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
            params![
                r.id, chapter_id, r.item_no, file_path, rel_path, r.sha256, r.size_bytes,
                r.duration, serde_json::to_string(&r.peaks).unwrap_or_else(|_| "[]".into()),
                r.imported_at, r.mtime
            ],
        )?;
        report.recordings_added += 1;
    }
    Ok(())
}

fn import_occurrences(conn: &Connection, snap: &Snapshot, report: &mut MergeReport) -> Result<()> {
    for o in &snap.occurrences {
        if id_exists(conn, "recording_occurrence", &o.id)? {
            continue;
        }
        if !id_exists(conn, "recording", &o.recording_id)? || !id_exists(conn, "entry", &o.entry_id)? {
            report.skipped.push(format!("出现位置 {} 依赖的录音/词条缺失，已跳过", o.id));
            continue;
        }
        conn.execute(
            "INSERT INTO recording_occurrence(id, recording_id, entry_id, role_ref, time_start, time_end)
             VALUES(?1,?2,?3,?4,?5,?6)",
            params![o.id, o.recording_id, o.entry_id, o.role_ref, o.time_start, o.time_end],
        )?;
    }
    Ok(())
}

fn import_annotations(conn: &Connection, snap: &Snapshot, report: &mut MergeReport) -> Result<()> {
    for a in &snap.annotations {
        if id_exists(conn, "annotation", &a.id)? {
            // 同一 id：内容一致则幂等跳过；不一致记冲突，绝不覆盖。
            let (local_comment, local_start): (String, f64) = conn.query_row(
                "SELECT comment, time_start FROM annotation WHERE id=?1",
                [&a.id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            if local_comment != a.comment || (local_start - a.time_start).abs() > 1e-9 {
                report.conflicts.push(MergeConflict {
                    kind: "annotation_divergent".into(),
                    id: a.id.clone(),
                    detail: format!("批注 {} 内容不同，保留本地版本", a.id),
                });
                report.annotations_conflicted += 1;
            }
            continue;
        }
        if !id_exists(conn, "recording", &a.recording_id)? {
            report.skipped.push(format!("批注 {} 的录音缺失，已跳过", a.id));
            continue;
        }
        if let Some(vid) = &a.entry_version_id {
            if !id_exists(conn, "entry_version", vid)? {
                report.skipped.push(format!("批注 {} 指向的词典版本缺失，已跳过", a.id));
                continue;
            }
        }
        // 越界校验：包来自别人的工程或录音被替换过，时间段可能已无效。
        if let Err(e) = validate_window(a.time_start, a.time_end) {
            report.conflicts.push(MergeConflict {
                kind: "annotation_out_of_bounds".into(),
                id: a.id.clone(),
                detail: e.to_string(),
            });
            report.annotations_conflicted += 1;
            continue;
        }
        let duration: f64 = conn.query_row(
            "SELECT duration FROM recording WHERE id=?1",
            [&a.recording_id],
            |r| r.get(0),
        )?;
        if a.time_end > duration + 0.001 {
            report.conflicts.push(MergeConflict {
                kind: "annotation_out_of_bounds".into(),
                id: a.id.clone(),
                detail: format!(
                    "批注 {}s–{}s 超出片段时长 {}s",
                    a.time_start, a.time_end, duration
                ),
            });
            report.annotations_conflicted += 1;
            continue;
        }
        conn.execute(
            "INSERT INTO annotation(id, recording_id, kind, time_start, time_end, comment,
                                    entry_version_id, expected_ipa, reviewer, created_at, resolved)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
            params![
                a.id, a.recording_id, a.kind, a.time_start, a.time_end, a.comment,
                a.entry_version_id, a.expected_ipa, a.reviewer, a.created_at, a.resolved
            ],
        )?;
        report.annotations_added += 1;

        // 未解决的外来批注 → 本地产生一条待复核范围，提示需要听这段。
        if a.resolved == 0 {
            let range_id = uuid::Uuid::new_v4().to_string();
            let detail = format!("来自 {} 的批注（{}）", a.reviewer, a.kind);
            conn.execute(
                "INSERT INTO review_range(id, recording_id, entry_id, time_start, time_end, reason, detail,
                                          from_version_id, to_version_id, status, created_at)
                 VALUES(?1,?2,?3,?4,?5,'imported_annotation',?6,?7,?8,'pending',?9)
                 ON CONFLICT(recording_id, COALESCE(entry_id, ''), time_start, time_end, reason)
                 DO UPDATE SET detail=CASE WHEN review_range.detail LIKE '%' || excluded.detail || '%'
                                           THEN review_range.detail
                                           ELSE review_range.detail || '; ' || excluded.detail END
                 WHERE review_range.status='pending'",
                params![
                    range_id, a.recording_id, None::<String>,
                    a.time_start, a.time_end, detail, None::<String>, None::<String>, now_ts()
                ],
            )?;
            report.review_ranges_added += conn.changes() as usize;
        }
    }
    Ok(())
}

fn import_review_ranges(conn: &Connection, snap: &Snapshot, report: &mut MergeReport) -> Result<()> {
    for r in &snap.review_ranges {
        if r.status != "pending" || id_exists(conn, "review_range", &r.id)? {
            continue;
        }
        if !id_exists(conn, "recording", &r.recording_id)? {
            continue;
        }
        conn.execute(
            "INSERT INTO review_range(id, recording_id, entry_id, time_start, time_end, reason, detail,
                                      from_version_id, to_version_id, status, created_at, resolved_by, resolution)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,'pending',?10,NULL,NULL)
             ON CONFLICT DO NOTHING",
            params![
                r.id, r.recording_id, r.entry_id, r.time_start, r.time_end, r.reason, r.detail,
                r.from_version_id, r.to_version_id, r.created_at
            ],
        )?;
        report.review_ranges_added += conn.changes() as usize;
    }
    Ok(())
}
