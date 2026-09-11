// Prevents an extra console window on Windows in release.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Tauri 命令层：每个命令都是 ab-core 领域接口的薄封装。
//! 所有状态保存在 SQLite，前端只拿到 DTO。

use std::path::PathBuf;
use std::sync::Mutex;

use ab_core::db::Studio;
use ab_core::diff::{diff_snapshots, load_snapshot, load_snapshot_file, render_text_report};
use ab_core::models::*;
use serde::Serialize;
use tauri::State;

type CmdResult<T> = std::result::Result<T, String>;

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

struct AppState {
    studio: Mutex<Option<Studio>>,
}

fn with_studio<T>(state: &State<AppState>, f: impl FnOnce(&Studio) -> ab_core::Result<T>) -> CmdResult<T> {
    let guard = state.studio.lock().unwrap();
    match guard.as_ref() {
        Some(s) => f(s).map_err(err),
        None => Err("还没有打开工程".into()),
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MediaPayload {
    bytes: Vec<u8>,
    file_name: String,
}

// ---- 工程 -----------------------------------------------------------------

#[tauri::command]
fn open_project(state: State<AppState>, dir: String) -> CmdResult<ProjectInfo> {
    let studio = Studio::open(&dir).map_err(err)?;
    let info = studio.project_info().map_err(err)?;
    *state.studio.lock().unwrap() = Some(studio);
    Ok(info)
}

#[tauri::command]
fn current_project(state: State<AppState>) -> CmdResult<Option<ProjectInfo>> {
    Ok(match state.studio.lock().unwrap().as_ref() {
        Some(s) => Some(s.project_info().map_err(err)?),
        None => None,
    })
}

#[tauri::command]
fn rename_project(state: State<AppState>, name: String) -> CmdResult<()> {
    with_studio(&state, |s| s.set_project_name(&name))
}

// ---- 词典 -----------------------------------------------------------------

#[tauri::command]
fn list_entries(state: State<AppState>) -> CmdResult<Vec<Entry>> {
    with_studio(&state, |s| s.list_entries())
}

#[tauri::command]
fn create_entry(state: State<AppState>, input: NewEntry) -> CmdResult<(Entry, EntryVersion)> {
    with_studio(&state, |s| s.create_entry(input))
}

#[tauri::command]
fn list_versions(state: State<AppState>, entry_id: String) -> CmdResult<Vec<EntryVersion>> {
    with_studio(&state, |s| s.list_versions(&entry_id))
}

#[tauri::command]
fn list_all_versions(state: State<AppState>) -> CmdResult<Vec<EntryVersion>> {
    with_studio(&state, |s| s.list_all_versions())
}

#[tauri::command]
fn add_version(
    state: State<AppState>,
    entry_id: String,
    input: NewVersion,
) -> CmdResult<(EntryVersion, usize)> {
    with_studio(&state, |s| s.add_version(&entry_id, input))
}

#[tauri::command]
fn approve_version(state: State<AppState>, version_id: String) -> CmdResult<usize> {
    with_studio(&state, |s| s.approve_version(&version_id))
}

#[tauri::command]
fn reject_version(state: State<AppState>, version_id: String) -> CmdResult<()> {
    with_studio(&state, |s| s.reject_version(&version_id))
}

#[tauri::command]
fn list_exceptions(state: State<AppState>, entry_id: String) -> CmdResult<Vec<EntryException>> {
    with_studio(&state, |s| s.list_exceptions(&entry_id))
}

#[tauri::command]
fn upsert_exception(
    state: State<AppState>,
    entry_id: String,
    input: NewException,
) -> CmdResult<(EntryException, usize)> {
    with_studio(&state, |s| s.upsert_exception(&entry_id, input))
}

#[tauri::command]
fn delete_exception(state: State<AppState>, exception_id: String) -> CmdResult<()> {
    with_studio(&state, |s| s.delete_exception(&exception_id))
}

#[tauri::command]
fn resolve_pron(state: State<AppState>, query: ResolveQuery) -> CmdResult<ResolvedPron> {
    with_studio(&state, |s| s.resolve(&query))
}

// ---- 录音 -----------------------------------------------------------------

#[tauri::command]
fn list_chapters(state: State<AppState>) -> CmdResult<Vec<Chapter>> {
    with_studio(&state, |s| s.list_chapters())
}

#[tauri::command]
fn list_recordings(state: State<AppState>) -> CmdResult<Vec<Recording>> {
    with_studio(&state, |s| s.list_recordings())
}

/// 导入录音。前端用 Web Audio 解码得到时长（非 WAV 时），并负责用文件对话框选路径。
/// 探测源音频时长（WAV 后端解析；非 WAV 返回 null，由前端 Web Audio 解码）。
#[tauri::command]
fn probe_audio_duration(path: String) -> CmdResult<Option<f64>> {
    let bytes = std::fs::read(&path).map_err(err)?;
    Ok(ab_core::wav::parse(&bytes).map(|i| i.duration))
}

/// 读取任意源文件字节（前端用 decodeAudioData 探测非 WAV 时长用）。
#[tauri::command]
fn read_file_bytes(path: String) -> CmdResult<Vec<u8>> {
    std::fs::read(&path).map_err(err)
}

#[tauri::command]
fn import_recording(state: State<AppState>, input: NewRecording) -> CmdResult<Recording> {
    with_studio(&state, |s| s.import_recording(input))
}

#[tauri::command]
fn verify_recording(state: State<AppState>, recording_id: String) -> CmdResult<VerifyResult> {
    with_studio(&state, |s| s.verify_recording(&recording_id))
}

/// 读取媒体字节交给前端 Web Audio 解码播放。
#[tauri::command]
fn read_media(state: State<AppState>, recording_id: String) -> CmdResult<MediaPayload> {
    with_studio(&state, |s| {
        let rec = s.get_recording(&recording_id)?;
        let bytes = std::fs::read(&rec.file_path)?;
        let name = PathBuf::from(&rec.file_path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "audio".into());
        Ok(MediaPayload { bytes, file_name: name })
    })
}

#[tauri::command]
fn add_occurrence(state: State<AppState>, input: NewOccurrence) -> CmdResult<RecordingOccurrence> {
    with_studio(&state, |s| s.add_occurrence(input))
}

#[tauri::command]
fn list_occurrences(state: State<AppState>, recording_id: String) -> CmdResult<Vec<RecordingOccurrence>> {
    with_studio(&state, |s| s.list_occurrences(&recording_id))
}

#[tauri::command]
fn remove_occurrence(state: State<AppState>, id: String) -> CmdResult<()> {
    with_studio(&state, |s| s.remove_occurrence(&id))
}

// ---- 批注 / 复核 ----------------------------------------------------------

#[tauri::command]
fn list_annotations(state: State<AppState>, recording_id: String) -> CmdResult<Vec<Annotation>> {
    with_studio(&state, |s| s.list_annotations(&recording_id))
}

#[tauri::command]
fn add_annotation(state: State<AppState>, input: NewAnnotation) -> CmdResult<Annotation> {
    with_studio(&state, |s| s.add_annotation(input))
}

#[tauri::command]
fn resolve_annotation(state: State<AppState>, id: String) -> CmdResult<()> {
    with_studio(&state, |s| s.resolve_annotation(&id))
}

#[tauri::command]
fn list_pending_reviews(state: State<AppState>) -> CmdResult<Vec<ReviewRange>> {
    with_studio(&state, |s| s.list_pending_reviews())
}

#[tauri::command]
fn list_recording_reviews(state: State<AppState>, recording_id: String) -> CmdResult<Vec<ReviewRange>> {
    with_studio(&state, |s| s.list_reviews_for_recording(&recording_id))
}

#[tauri::command]
fn resolve_review(state: State<AppState>, id: String, resolution: String, reviewer: String) -> CmdResult<()> {
    with_studio(&state, |s| s.resolve_review(&id, &resolution, &reviewer))
}

// ---- 离线包 / 差异 --------------------------------------------------------

#[tauri::command]
fn export_package(
    state: State<AppState>,
    out_path: String,
    options: PackageOptions,
    exported_by: Option<String>,
) -> CmdResult<String> {
    with_studio(&state, |s| s.export_package(&out_path, &options, exported_by.as_deref()))
}

#[tauri::command]
fn import_package(state: State<AppState>, path: String) -> CmdResult<MergeReport> {
    with_studio(&state, |s| s.import_package(&path))
}

#[tauri::command]
fn build_snapshot(state: State<AppState>, options: PackageOptions) -> CmdResult<Snapshot> {
    with_studio(&state, |s| s.snapshot(&options))
}

/// 比较当前工程与另一份快照/离线包，返回结构化报告。
#[tauri::command]
fn diff_against_file(state: State<AppState>, path: String) -> CmdResult<DiffReport> {
    with_studio(&state, |s| {
        let base = s.snapshot(&PackageOptions { include_media: false, reviewer_filter: None })?;
        let target = load_snapshot_file(&path)?;
        Ok(diff_snapshots(&base, &target))
    })
}

/// 比较两份 .abpkg / snapshot.json 文件（当前工程可以尚未打开）。
#[tauri::command]
fn diff_files(base_path: String, target_path: String) -> CmdResult<String> {
    let base = load_snapshot_file(&base_path).map_err(err)?;
    let target = load_snapshot_file(&target_path).map_err(err)?;
    Ok(render_text_report(&diff_snapshots(&base, &target)))
}

#[tauri::command]
fn parse_snapshot_bytes(bytes: Vec<u8>) -> CmdResult<Snapshot> {
    load_snapshot(&bytes).map_err(err)
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState { studio: Mutex::new(None) })
        .invoke_handler(tauri::generate_handler![
            open_project,
            current_project,
            rename_project,
            list_entries,
            create_entry,
            list_versions,
            list_all_versions,
            add_version,
            approve_version,
            reject_version,
            list_exceptions,
            upsert_exception,
            delete_exception,
            resolve_pron,
            list_chapters,
            list_recordings,
            probe_audio_duration,
            read_file_bytes,
            import_recording,
            verify_recording,
            read_media,
            add_occurrence,
            list_occurrences,
            remove_occurrence,
            list_annotations,
            add_annotation,
            resolve_annotation,
            list_pending_reviews,
            list_recording_reviews,
            resolve_review,
            export_package,
            import_package,
            build_snapshot,
            diff_against_file,
            diff_files,
            parse_snapshot_bytes
        ])
        .run(tauri::generate_context!())
        .expect("启动有声书工坊失败");
}
