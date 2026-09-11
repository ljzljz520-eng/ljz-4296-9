//! 音频被外部替换、批注越界。

use ab_core::models::*;
use ab_core::{wav, Error, Studio};

fn make_entry(s: &Studio) -> String {
    s.create_entry(NewEntry {
        headword: "龟兹".into(),
        scope: "第1章".into(),
        kind: "place".into(),
        tags: String::new(),
        ipa: "qiū cí".into(),
        syllabification: String::new(),
        example_audio: None,
        source: String::new(),
        approved: true,
        note: String::new(),
    })
    .unwrap()
    .0
    .id
}

#[test]
fn annotation_outside_duration_is_rejected() {
    let s = Studio::in_memory().unwrap();
    let p = std::env::temp_dir().join("short.wav");
    std::fs::write(&p, wav::make_wav(16000, 2.0, 0.5)).unwrap();
    let rec = s
        .import_recording(NewRecording {
            chapter_no: 1,
            chapter_title: String::new(),
            item_no: "1".into(),
            src_path: p.to_string_lossy().into(),
            duration: None,
            copy_into_project: false,
        })
        .unwrap();

    let err = s
        .add_annotation(NewAnnotation {
            recording_id: rec.id.clone(),
            kind: AnnotationKind::Mispron,
            time_start: 1.9,
            time_end: 2.5,
            comment: "越界".into(),
            entry_version_id: None,
            reviewer: Some("甲".into()),
        })
        .unwrap_err();
    assert!(matches!(err, Error::OutOfBounds(..)), "实际: {err:?}");

    // start >= end 也算非法窗口
    let err = s
        .add_annotation(NewAnnotation {
            recording_id: rec.id.clone(),
            kind: AnnotationKind::Noise,
            time_start: 1.5,
            time_end: 1.5,
            comment: "零长度".into(),
            entry_version_id: None,
            reviewer: None,
        })
        .unwrap_err();
    assert!(matches!(err, Error::BadRange(..)));

    // 合法批注可以进
    s.add_annotation(NewAnnotation {
        recording_id: rec.id.clone(),
        kind: AnnotationKind::Stress,
        time_start: 0.1,
        time_end: 0.4,
        comment: "重音".into(),
        entry_version_id: None,
        reviewer: None,
    })
    .unwrap();
    assert_eq!(s.list_annotations(&rec.id).unwrap().len(), 1);
}

#[test]
fn external_replacement_is_detected_and_oob_annotations_listed() {
    let s = Studio::in_memory().unwrap();
    let entry = make_entry(&s);

    // 录音被复制进工程，外部原件删掉也不影响；这里直接改工程内文件模拟“被外部替换”。
    let src = std::env::temp_dir().join("rep.wav");
    std::fs::write(&src, wav::make_wav(16000, 5.0, 0.5)).unwrap();
    let rec = s
        .import_recording(NewRecording {
            chapter_no: 1,
            chapter_title: String::new(),
            item_no: "3".into(),
            src_path: src.to_string_lossy().into(),
            duration: None,
            copy_into_project: true,
        })
        .unwrap();
    let old_sha = rec.sha256.clone();
    let stored = rec.file_path.clone();

    // 在 4.5s 处放一条批注（5s 文件内合法）
    s.add_annotation(NewAnnotation {
        recording_id: rec.id.clone(),
        kind: AnnotationKind::Mispron,
        time_start: 4.4,
        time_end: 4.9,
        comment: "qiū 读成 guī".into(),
        entry_version_id: Some(s.list_versions(&entry).unwrap()[0].id.clone()),
        reviewer: Some("审听甲".into()),
    })
    .unwrap();

    // 未替换：changed=false
    let v = s.verify_recording(&rec.id).unwrap();
    assert!(!v.changed);
    assert!(v.exists);

    // 外部用 2s 的同文件名文件替换
    std::fs::write(&stored, wav::make_wav(16000, 2.0, 0.8)).unwrap();
    let v = s.verify_recording(&rec.id).unwrap();
    assert!(v.changed, "应检测到替换");
    assert_ne!(v.old_sha, v.new_sha.as_deref().unwrap());
    assert_eq!(v.old_sha, old_sha);
    assert_eq!(v.oob_annotations.len(), 1, "旧批注超出新时长");
    assert!((v.oob_annotations[0].time_end - 4.9).abs() < 1e-9);

    // 越界批注不会被自动删除（交给审听者处理）
    assert_eq!(s.list_annotations(&rec.id).unwrap().len(), 1);

    // 摘要已刷新
    let reloaded = s.get_recording(&rec.id).unwrap();
    assert!((reloaded.duration - 2.0).abs() < 1e-6);
    assert_ne!(reloaded.sha256, old_sha);

    // 文件丢失
    std::fs::remove_file(&stored).unwrap();
    let v = s.verify_recording(&rec.id).unwrap();
    assert!(!v.exists);
    assert!(!v.changed);
}

#[test]
fn unchanged_file_passes_and_sha_is_stable() {
    let s = Studio::in_memory().unwrap();
    let src = std::env::temp_dir().join("same.wav");
    std::fs::write(&src, wav::make_wav(8000, 1.0, 0.3)).unwrap();
    let rec = s
        .import_recording(NewRecording {
            chapter_no: 3,
            chapter_title: "第三章".into(),
            item_no: "12b".into(),
            src_path: src.to_string_lossy().into(),
            duration: None,
            copy_into_project: true,
        })
        .unwrap();
    let sha = rec.sha256.clone();
    let v = s.verify_recording(&rec.id).unwrap();
    assert!(!v.changed);
    assert_eq!(v.new_sha.unwrap(), sha);
    assert_eq!(sha.len(), 64);
}
