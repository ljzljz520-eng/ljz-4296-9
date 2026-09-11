//! 端到端：WAV 元数据、改读音 → 待复核 → 处置 → 差异报告。

use ab_core::diff::{diff_snapshots, render_text_report};
use ab_core::models::*;
use ab_core::{wav, Studio};

#[test]
fn imported_wav_records_duration_peaks_and_copies_media() {
    let s = Studio::in_memory().unwrap();
    let src = std::env::temp_dir().join(format!("e2e-{}.wav", std::process::id()));
    std::fs::write(&src, wav::make_wav(22050, 2.5, 0.6)).unwrap();

    let rec = s
        .import_recording(NewRecording {
            chapter_no: 4,
            chapter_title: "第四章".into(),
            item_no: "9".into(),
            src_path: src.to_string_lossy().into(),
            duration: None,
            copy_into_project: true,
        })
        .unwrap();
    assert!((rec.duration - 2.5).abs() < 0.05);
    assert!(rec.peaks.len() > 10, "应生成峰值桶，实际 {}", rec.peaks.len());
    assert!(rec.rel_path.is_some());
    assert!(s.root.join(rec.rel_path.unwrap()).exists());
    assert_eq!(rec.size_bytes as usize, std::fs::read(&src).unwrap().len());

    // 章节与录音可查
    assert_eq!(s.list_chapters().unwrap()[0].chapter_no, 4);
    assert_eq!(s.list_recordings().unwrap().len(), 1);
}

#[test]
fn pronunciation_change_flows_into_diff_report() {
    let s = Studio::in_memory().unwrap();
    let entry = s
        .create_entry(NewEntry {
            headword: "单于".into(),
            scope: "全书".into(),
            kind: "character".into(),
            tags: String::new(),
            ipa: "chán yú".into(),
            syllabification: String::new(),
            example_audio: None,
            source: String::new(),
            approved: true,
            note: String::new(),
        })
        .unwrap()
        .0;
    let src = std::env::temp_dir().join(format!("e2e2-{}.wav", std::process::id()));
    std::fs::write(&src, wav::make_wav(16000, 3.0, 0.5)).unwrap();
    let rec = s
        .import_recording(NewRecording {
            chapter_no: 1,
            chapter_title: String::new(),
            item_no: "1".into(),
            src_path: src.to_string_lossy().into(),
            duration: None,
            copy_into_project: false,
        })
        .unwrap();
    let rec_id = rec.id.clone();
    s.add_occurrence(NewOccurrence {
        recording_id: rec_id.clone(),
        headword: "单于".into(),
        chapter: Some(1),
        role: None,
        time_start: 0.2,
        time_end: 0.7,
    })
    .unwrap();

    let before = s.snapshot(&PackageOptions { include_media: false, reviewer_filter: None }).unwrap();

    // 审听批注：误读，指向 v1
    let v1 = s.list_versions(&entry.id).unwrap()[0].id.clone();
    s.add_annotation(NewAnnotation {
        recording_id: rec_id,
        kind: AnnotationKind::Mispron,
        time_start: 0.2,
        time_end: 0.6,
        comment: "读成了 dān yú".into(),
        entry_version_id: Some(v1),
        reviewer: Some("甲".into()),
    })
    .unwrap();

    // 改默认读音 → 待复核
    let (_v2, n) = s
        .add_version(
            &entry.id,
            NewVersion {
                ipa: "shàn yú".into(),
                syllabification: String::new(),
                example_audio: None,
                source: "作者二次确认".into(),
                note: String::new(),
                approved: true,
            },
        )
        .unwrap();
    assert_eq!(n, 1);
    let pending = s.list_pending_reviews().unwrap();
    s.resolve_review(&pending[0].id, "rerecord", "乙").unwrap();

    let after = s.snapshot(&PackageOptions { include_media: false, reviewer_filter: None }).unwrap();
    let d = diff_snapshots(&before, &after);
    assert_eq!(d.entries_changed.len(), 1);
    assert!(d.entries_changed[0].detail.contains("chán yú"));
    assert!(d.entries_changed[0].detail.contains("shàn yú"));
    assert_eq!(d.annotations_added.len(), 1);
    assert_eq!(d.review_resolved.len(), 1);
    assert!(d.review_resolved[0].detail.contains("重录"));

    let text = render_text_report(&d);
    assert!(text.contains("单于"));
    assert!(text.contains("需重录"));
    assert!(text.contains("新增批注"));
}

#[test]
fn revision_bumps_on_structural_changes() {
    let s = Studio::in_memory().unwrap();
    let r0 = s.revision().unwrap();
    let (e, _) = s
        .create_entry(NewEntry {
            headword: "x".into(), scope: "全书".into(), kind: "term".into(), tags: String::new(),
            ipa: "a".into(), syllabification: String::new(), example_audio: None,
            source: String::new(), approved: true, note: String::new(),
        })
        .unwrap();
    assert_eq!(s.revision().unwrap(), r0 + 1);
    s.upsert_exception(
        &e.id,
        NewException {
            scope_kind: "chapter".into(), scope_ref: "1".into(),
            ipa: "b".into(), syllabification: String::new(), note: String::new(),
        },
    )
    .unwrap();
    assert_eq!(s.revision().unwrap(), r0 + 2);
}
