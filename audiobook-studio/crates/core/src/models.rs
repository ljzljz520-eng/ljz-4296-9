//! 与前端交换的 DTO（camelCase JSON）。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectInfo {
    pub name: String,
    pub revision: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub id: String,
    pub headword: String,
    /// 书目范围，如 "全书"、"第1-12章"
    pub scope: String,
    pub kind: String,
    pub tags: String,
    pub current_version_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryVersion {
    pub id: String,
    pub entry_id: String,
    pub version_no: i64,
    pub ipa: String,
    pub syllabification: String,
    pub example_audio: Option<String>,
    pub source: String,
    pub status: String,
    pub note: String,
    pub created_at: String,
    pub approved_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryException {
    pub id: String,
    pub entry_id: String,
    /// 作用类型：role（角色）/ chapter（章节）
    pub scope_kind: String,
    /// 角色名或章节号
    pub scope_ref: String,
    pub ipa: String,
    pub syllabification: String,
    pub note: String,
    pub created_at: String,
}

/// 解析同形词读音的输入。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveQuery {
    pub headword: String,
    pub chapter: Option<i64>,
    pub role: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedPron {
    pub entry_id: String,
    pub headword: String,
    pub version_id: String,
    pub ipa: String,
    pub syllabification: String,
    /// "default" / "exception:role" / "exception:chapter"
    pub matched: String,
    pub exception_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Chapter {
    pub id: String,
    pub chapter_no: i64,
    pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Recording {
    pub id: String,
    pub chapter_id: String,
    /// 条次，如 "12"、"12b"
    pub item_no: String,
    pub file_path: String,
    pub rel_path: Option<String>,
    pub sha256: String,
    pub size_bytes: i64,
    pub duration: f64,
    pub peaks: Vec<f32>,
    pub imported_at: String,
    pub mtime: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingOccurrence {
    pub id: String,
    pub recording_id: String,
    pub entry_id: String,
    /// 该次出现对应的角色（可空）
    pub role_ref: Option<String>,
    pub time_start: f64,
    pub time_end: f64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AnnotationKind {
    Mispron,
    Stress,
    Noise,
    Other,
}

impl AnnotationKind {
    pub fn as_str(self) -> &'static str {
        match self {
            AnnotationKind::Mispron => "mispron",
            AnnotationKind::Stress => "stress",
            AnnotationKind::Noise => "noise",
            AnnotationKind::Other => "other",
        }
    }
    pub fn parse(s: &str) -> Self {
        match s {
            "mispron" => AnnotationKind::Mispron,
            "stress" => AnnotationKind::Stress,
            "noise" => AnnotationKind::Noise,
            _ => AnnotationKind::Other,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Annotation {
    pub id: String,
    pub recording_id: String,
    pub kind: String,
    pub time_start: f64,
    pub time_end: f64,
    pub comment: String,
    /// 批注时锁定的词典版本 ID（审听意见指向词典版本）
    pub entry_version_id: Option<String>,
    pub expected_ipa: Option<String>,
    pub reviewer: String,
    pub created_at: String,
    pub resolved: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewAnnotation {
    pub recording_id: String,
    pub kind: AnnotationKind,
    pub time_start: f64,
    pub time_end: f64,
    pub comment: String,
    #[serde(default)]
    pub entry_version_id: Option<String>,
    #[serde(default)]
    pub reviewer: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewRange {
    pub id: String,
    pub recording_id: String,
    pub entry_id: Option<String>,
    pub time_start: f64,
    pub time_end: f64,
    /// "pron_changed" / "imported_annotation"
    pub reason: String,
    pub detail: String,
    pub from_version_id: Option<String>,
    pub to_version_id: Option<String>,
    pub status: String,
    pub created_at: String,
    pub resolved_by: Option<String>,
    pub resolution: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyResult {
    pub recording_id: String,
    pub changed: bool,
    pub old_sha: String,
    pub new_sha: Option<String>,
    pub exists: bool,
    pub old_duration: f64,
    pub new_duration: Option<f64>,
    /// 超出新时长的批注
    pub oob_annotations: Vec<Annotation>,
    pub note: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewEntry {
    pub headword: String,
    #[serde(default = "default_scope")]
    pub scope: String,
    #[serde(default = "default_kind")]
    pub kind: String,
    #[serde(default)]
    pub tags: String,
    pub ipa: String,
    #[serde(default)]
    pub syllabification: String,
    #[serde(default)]
    pub example_audio: Option<String>,
    #[serde(default)]
    pub source: String,
    #[serde(default = "default_approved")]
    pub approved: bool,
    #[serde(default)]
    pub note: String,
}

fn default_scope() -> String {
    "全书".into()
}
fn default_kind() -> String {
    "term".into()
}
fn default_approved() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewVersion {
    pub ipa: String,
    #[serde(default)]
    pub syllabification: String,
    #[serde(default)]
    pub example_audio: Option<String>,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub note: String,
    /// 是否直接批准。未批准的新版本不会成为当前版本，也不会产生待复核范围。
    #[serde(default = "default_approved")]
    pub approved: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewException {
    pub scope_kind: String,
    pub scope_ref: String,
    #[serde(default)]
    pub ipa: String,
    #[serde(default)]
    pub syllabification: String,
    #[serde(default)]
    pub note: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewRecording {
    pub chapter_no: i64,
    #[serde(default)]
    pub chapter_title: String,
    pub item_no: String,
    /// 源文件绝对路径
    pub src_path: String,
    /// Web Audio 解码得到的时长（非 WAV 时必需）
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<f64>,
    /// 是否把媒体复制进工程 media 目录（离线打包需要）
    #[serde(default = "default_true")]
    pub copy_into_project: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewOccurrence {
    pub recording_id: String,
    pub headword: String,
    pub chapter: Option<i64>,
    pub role: Option<String>,
    pub time_start: f64,
    pub time_end: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MergeReport {
    pub packages_applied: Vec<String>,
    pub recordings_added: usize,
    pub annotations_added: usize,
    pub annotations_conflicted: usize,
    pub versions_added: usize,
    pub exceptions_added: usize,
    pub review_ranges_added: usize,
    pub skipped: Vec<String>,
    pub conflicts: Vec<MergeConflict>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MergeConflict {
    pub kind: String,
    pub id: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageOptions {
    #[serde(default = "default_true")]
    pub include_media: bool,
    #[serde(default)]
    pub reviewer_filter: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DiffReport {
    pub base_revision: Option<i64>,
    pub target_revision: Option<i64>,
    pub entries_added: Vec<DiffItem>,
    pub entries_changed: Vec<DiffItem>,
    pub recordings_added: Vec<DiffItem>,
    pub recordings_replaced: Vec<DiffItem>,
    pub annotations_added: Vec<DiffItem>,
    pub review_resolved: Vec<DiffItem>,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffItem {
    pub key: String,
    pub title: String,
    pub detail: String,
}

/// 导出/差异用的工程快照（同时也是离线包内 snapshot.json 的结构）。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub format: i64,
    pub project: String,
    pub revision: i64,
    pub exported_at: String,
    pub exported_by: Option<String>,
    pub source_path: Option<String>,
    pub entries: Vec<Entry>,
    pub versions: Vec<EntryVersion>,
    pub exceptions: Vec<EntryException>,
    pub chapters: Vec<Chapter>,
    pub recordings: Vec<Recording>,
    pub occurrences: Vec<RecordingOccurrence>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub annotations: Vec<Annotation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub review_ranges: Vec<ReviewRange>,
    #[serde(default, skip_serializing_if = "usize_is_zero")]
    pub media_count: usize,
}

/// 让 skip_serializing_if 可用于 usize。
fn usize_is_zero(v: &usize) -> bool {
    *v == 0
}
