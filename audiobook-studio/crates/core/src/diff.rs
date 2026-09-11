//! 工程快照差异：比较两份 snapshot.json（也支持工程与包互比），生成中文报告。

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use crate::error::{Error, Result};
use crate::models::*;

/// 读取快照：.abpkg(zip) 或裸 snapshot.json 均可。
pub fn load_snapshot(bytes: &[u8]) -> Result<Snapshot> {
    if bytes.starts_with(b"PK\x03\x04") {
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes))
            .map_err(|e| Error::Package(e.to_string()))?;
        let mut f = zip
            .by_name("snapshot.json")
            .map_err(|e| Error::Package(format!("包内缺少 snapshot.json: {e}")))?;
        let mut s = String::new();
        std::io::Read::read_to_string(&mut f, &mut s)?;
        Ok(serde_json::from_str(&s)?)
    } else {
        Ok(serde_json::from_slice(bytes)?)
    }
}

pub fn load_snapshot_file(path: impl AsRef<Path>) -> Result<Snapshot> {
    load_snapshot(&std::fs::read(path)?)
}

/// 比较 base -> target。
pub fn diff_snapshots(base: &Snapshot, target: &Snapshot) -> DiffReport {
    let mut rep = DiffReport {
        base_revision: Some(base.revision),
        target_revision: Some(target.revision),
        ..Default::default()
    };

    let base_entries: HashMap<&str, &Entry> = base.entries.iter().map(|e| (e.id.as_str(), e)).collect();
    let base_versions: HashMap<&str, &EntryVersion> =
        base.versions.iter().map(|v| (v.id.as_str(), v)).collect();

    for e in &target.entries {
        match base_entries.get(e.id.as_str()) {
            None => rep.entries_added.push(DiffItem {
                key: e.id.clone(),
                title: e.headword.clone(),
                detail: format!("新{}「{}」({})", kind_label(&e.kind), e.headword, e.scope),
            }),
            Some(old) => {
                if old.current_version_id != e.current_version_id {
                    let old_v = old.current_version_id.as_ref().and_then(|id| base_versions.get(id.as_str()));
                    let new_v = target
                        .versions
                        .iter()
                        .find(|v| Some(&v.id) == e.current_version_id.as_ref());
                    rep.entries_changed.push(DiffItem {
                        key: e.id.clone(),
                        title: e.headword.clone(),
                        detail: format!(
                            "读音 v{} [{}] → v{} [{}]",
                            old_v.map(|v| v.version_no).unwrap_or(0),
                            old_v.map(|v| v.ipa.as_str()).unwrap_or("—"),
                            new_v.map(|v| v.version_no).unwrap_or(0),
                            new_v.map(|v| v.ipa.as_str()).unwrap_or("—"),
                        ),
                    });
                }
            }
        }
    }

    // 新增例外
    let base_exc: std::collections::HashSet<(String, String, String)> = base
        .exceptions
        .iter()
        .map(|x| (x.entry_id.clone(), x.scope_kind.clone(), x.scope_ref.clone()))
        .collect();
    for x in &target.exceptions {
        if !base_exc.contains(&(x.entry_id.clone(), x.scope_kind.clone(), x.scope_ref.clone())) {
            let head = entry_head(base, target, &x.entry_id);
            rep.entries_changed.push(DiffItem {
                key: x.id.clone(),
                title: head,
                detail: format!("新增{}例外「{}」: [{}]", scope_label(&x.scope_kind), x.scope_ref, x.ipa),
            });
        }
    }

    let base_rec: HashMap<&str, &Recording> =
        base.recordings.iter().map(|r| (r.id.as_str(), r)).collect();
    for r in &target.recordings {
        match base_rec.get(r.id.as_str()) {
            None => rep.recordings_added.push(DiffItem {
                key: r.id.clone(),
                title: recording_title(target, r),
                detail: format!("时长 {:.1}s, {}", r.duration, human_bytes(r.size_bytes)),
            }),
            Some(old) if old.sha256 != r.sha256 => rep.recordings_replaced.push(DiffItem {
                key: r.id.clone(),
                title: recording_title(target, r),
                detail: format!(
                    "文件被替换：{:.1}s→{:.1}s ({}→{})",
                    old.duration, r.duration, &old.sha256[..8], &r.sha256[..8]
                ),
            }),
            _ => {}
        }
    }

    let base_ann: HashMap<&str, &Annotation> =
        base.annotations.iter().map(|a| (a.id.as_str(), a)).collect();
    let by_reviewer = |a: &Annotation| a.reviewer.clone();
    let mut reviewer_added: BTreeMap<String, usize> = BTreeMap::new();
    for a in &target.annotations {
        if !base_ann.contains_key(a.id.as_str()) {
            rep.annotations_added.push(DiffItem {
                key: a.id.clone(),
                title: annotation_title(target, a),
                detail: format!(
                    "[{}] {} {:.2}–{:.2}s: {}{}",
                    a.reviewer,
                    kind_label(&a.kind),
                    a.time_start,
                    a.time_end,
                    a.comment,
                    a.expected_ipa.as_ref().map(|p| format!("（应读 {p}）")).unwrap_or_default()
                ),
            });
            *reviewer_added.entry(by_reviewer(a)).or_default() += 1;
        }
    }

    let base_ranges: HashMap<&str, &ReviewRange> =
        base.review_ranges.iter().map(|r| (r.id.as_str(), r)).collect();
    for r in &target.review_ranges {
        let was_pending = match base_ranges.get(r.id.as_str()) {
            Some(old) => old.status == "pending",
            // 基线里还不存在（本次修订期间新建并马上处置）也算一次处置
            None => true,
        };
        if was_pending && r.status != "pending" {
            rep.review_resolved.push(DiffItem {
                key: r.id.clone(),
                title: range_title(target, r),
                detail: format!(
                    "{} 处置：{}{}",
                    r.resolved_by.as_deref().unwrap_or("?"),
                    resolution_label(r.resolution.as_deref().unwrap_or("")),
                    if r.status == "rerecord" { "（需重录）" } else { "" }
                ),
            });
        }
    }

    let total = rep.entries_added.len()
        + rep.entries_changed.len()
        + rep.recordings_added.len()
        + rep.recordings_replaced.len()
        + rep.annotations_added.len()
        + rep.review_resolved.len();
    let reviewer_part = if reviewer_added.is_empty() {
        String::new()
    } else {
        let parts: Vec<String> = reviewer_added.iter().map(|(k, v)| format!("{k} {v} 条")).collect();
        format!("；新增批注按审听者：{}", parts.join("、"))
    };
    rep.summary = format!(
        "r{} → r{}：共 {total} 项变化（新增词条 {}、读音/例外变化 {}、新增录音 {}、录音替换 {}、新批注 {}、复核处置 {}{}）",
        base.revision, target.revision,
        rep.entries_added.len(), rep.entries_changed.len(),
        rep.recordings_added.len(), rep.recordings_replaced.len(),
        rep.annotations_added.len(), rep.review_resolved.len(), reviewer_part
    );
    rep
}

/// 渲染纯文本中文报告。
pub fn render_text_report(rep: &DiffReport) -> String {
    let mut out = String::new();
    out.push_str("========================================\n");
    out.push_str(" 有声书工坊 · 差异报告\n");
    out.push_str("========================================\n");
    out.push_str(&rep.summary);
    out.push('\n');

    let sections: [(&str, &[DiffItem]); 6] = [
        ("新增词条", &rep.entries_added),
        ("读音 / 例外变化", &rep.entries_changed),
        ("新增录音", &rep.recordings_added),
        ("被替换录音", &rep.recordings_replaced),
        ("新增批注", &rep.annotations_added),
        ("待复核处置", &rep.review_resolved),
    ];
    for (title, items) in sections {
        out.push_str(&format!("\n## {title}（{}）\n", items.len()));
        if items.is_empty() {
            out.push_str("（无）\n");
        } else {
            for it in items {
                out.push_str(&format!(" · {} — {}\n", it.title, it.detail));
            }
        }
    }
    out
}

fn entry_head(base: &Snapshot, target: &Snapshot, id: &str) -> String {
    target
        .entries
        .iter()
        .chain(base.entries.iter())
        .find(|e| e.id == id)
        .map(|e| e.headword.clone())
        .unwrap_or_else(|| id[..8].into())
}

fn recording_title(snap: &Snapshot, r: &Recording) -> String {
    let ch = snap.chapters.iter().find(|c| c.id == r.chapter_id).map(|c| c.chapter_no).unwrap_or(0);
    format!("第{ch}章·条次{}", r.item_no)
}

fn annotation_title(snap: &Snapshot, a: &Annotation) -> String {
    snap.recordings
        .iter()
        .find(|r| r.id == a.recording_id)
        .map(|r| recording_title(snap, r))
        .unwrap_or_else(|| a.recording_id[..8].into())
}

fn range_title(snap: &Snapshot, r: &ReviewRange) -> String {
    snap.recordings
        .iter()
        .find(|x| x.id == r.recording_id)
        .map(|x| recording_title(snap, x))
        .unwrap_or_else(|| r.recording_id[..8].into())
}

fn kind_label(k: &str) -> &'static str {
    match k {
        "character" => "角色",
        "place" => "地名",
        "term" => "术语",
        "mispron" => "误读",
        "stress" => "重音",
        "noise" => "噪声",
        _ => "其他",
    }
}

fn scope_label(k: &str) -> &str {
    match k {
        "role" => "角色",
        "chapter" => "章节",
        _ => "自定义",
    }
}

fn resolution_label(r: &str) -> &str {
    match r {
        "ok" => "旧录音可接受",
        "reannotated" => "已转为批注",
        "rerecord" => "需要重录",
        _ => "未知",
    }
}

fn human_bytes(n: i64) -> String {
    if n >= 1_000_000 {
        format!("{:.1} MB", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1} KB", n as f64 / 1_000.0)
    } else {
        format!("{n} B")
    }
}
