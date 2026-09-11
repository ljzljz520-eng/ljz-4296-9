//! 同名异读：同形词按角色/章节设例外，以及修改读音产生待复核而非判错。

use ab_core::models::*;
use ab_core::wav::make_wav;
use ab_core::Studio;

fn entry(head: &str, ipa: &str) -> NewEntry {
    NewEntry {
        headword: head.into(),
        scope: "全书".into(),
        kind: "term".into(),
        tags: String::new(),
        ipa: ipa.into(),
        syllabification: String::new(),
        example_audio: None,
        source: "导演组".into(),
        approved: true,
        note: String::new(),
    }
}

#[test]
fn duplicate_headword_is_rejected_use_exception_instead() {
    let s = Studio::in_memory().unwrap();
    s.create_entry(entry("楼兰", "lóu lán")).unwrap();
    let err = s.create_entry(entry("楼兰", "lǘ lán")).unwrap_err();
    assert!(err.to_string().contains("同名词条"), "实际: {err}");
}

#[test]
fn resolve_priority_role_over_chapter_over_default() {
    let s = Studio::in_memory().unwrap();
    let (e, _) = s.create_entry(entry("可汗", "kè hán")).unwrap();

    // 章节例外：第 3 章读 "kě hàn"
    s.upsert_exception(
        &e.id,
        NewException {
            scope_kind: "chapter".into(),
            scope_ref: "3".into(),
            ipa: "kě hàn".into(),
            syllabification: String::new(),
            note: String::new(),
        },
    )
    .unwrap();
    // 角色例外：旁白一律 "kě hán"（故意区分，验证优先级）
    s.upsert_exception(
        &e.id,
        NewException {
            scope_kind: "role".into(),
            scope_ref: "旁白".into(),
            ipa: "kě hán".into(),
            syllabification: String::new(),
            note: String::new(),
        },
    )
    .unwrap();

    let r = s
        .resolve(&ResolveQuery { headword: "可汗".into(), chapter: Some(5), role: None })
        .unwrap();
    assert_eq!(r.matched, "default");
    assert_eq!(r.ipa, "kè hán");

    // 仅章节命中
    let r = s
        .resolve(&ResolveQuery { headword: "可汗".into(), chapter: Some(3), role: None })
        .unwrap();
    assert_eq!(r.matched, "exception:chapter");
    assert_eq!(r.ipa, "kě hàn");

    // 角色 + 章节同时命中：角色优先
    let r = s
        .resolve(&ResolveQuery { headword: "可汗".into(), chapter: Some(3), role: Some("旁白".into()) })
        .unwrap();
    assert_eq!(r.matched, "exception:role");
    assert_eq!(r.ipa, "kě hán");
}

#[test]
fn changing_default_pronunciation_queues_only_default_occurrences() {
    let s = Studio::in_memory().unwrap();
    let (e, v1) = s.create_entry(entry("楼兰", "lóu lán")).unwrap();
    s.upsert_exception(
        &e.id,
        NewException {
            scope_kind: "role".into(),
            scope_ref: "旁白".into(),
            ipa: "lǘ lán".into(),
            syllabification: String::new(),
            note: String::new(),
        },
    )
    .unwrap();

    let wav = make_wav(16000, 4.0, 0.5);
    let path = std::env::temp_dir().join("a.wav");
    std::fs::write(&path, &wav).unwrap();
    let rec = s
        .import_recording(NewRecording {
            chapter_no: 1,
            chapter_title: String::new(),
            item_no: "1".into(),
            src_path: path.to_string_lossy().into(),
            duration: None,
            copy_into_project: true,
        })
        .unwrap();

    // 0.5s 处：普通读音；1.5s 处：旁白例外读音
    s.add_occurrence(NewOccurrence {
        recording_id: rec.id.clone(),
        headword: "楼兰".into(),
        chapter: Some(1),
        role: None,
        time_start: 0.4,
        time_end: 0.9,
    })
    .unwrap();
    s.add_occurrence(NewOccurrence {
        recording_id: rec.id.clone(),
        headword: "楼兰".into(),
        chapter: Some(1),
        role: Some("旁白".into()),
        time_start: 1.4,
        time_end: 1.9,
    })
    .unwrap();

    // 默认读音改版
    let (_v2, n) = s
        .add_version(
            &e.id,
            NewVersion {
                ipa: "luó lán".into(),
                syllabification: String::new(),
                example_audio: None,
                source: "校对".into(),
                note: String::new(),
                approved: true,
            },
        )
        .unwrap();
    assert_eq!(n, 1, "只有默认读音出现需要复核");

    let pending = s.list_pending_reviews().unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].reason, "pron_changed");
    assert_eq!(pending[0].from_version_id.as_deref(), Some(v1.id.as_str()));
    assert!((pending[0].time_start - 0.4).abs() < 1e-9);

    // 旧录音没有被自动判错：批注数为 0
    assert!(s.list_annotations(&rec.id).unwrap().is_empty());

    // 复核者确认旧录音可接受
    s.resolve_review(&pending[0].id, "ok", "审听甲").unwrap();
    assert!(s.list_pending_reviews().unwrap().is_empty());

    // 同样的读音再次改版：已处置的范围重新进入待复核
    let (_v3, n) = s
        .add_version(
            &e.id,
            NewVersion {
                ipa: "loú lǎn".into(),
                syllabification: String::new(),
                example_audio: None,
                source: "校对".into(),
                note: String::new(),
                approved: true,
            },
        )
        .unwrap();
    assert_eq!(n, 1);
}

#[test]
fn proposed_version_does_not_change_anything_until_approved() {
    let s = Studio::in_memory().unwrap();
    let (e, v1) = s.create_entry(entry("楼兰", "lóu lán")).unwrap();
    let (v2, n) = s
        .add_version(
            &e.id,
            NewVersion {
                ipa: "luó lán".into(),
                syllabification: String::new(),
                example_audio: None,
                source: String::new(),
                note: String::new(),
                approved: false,
            },
        )
        .unwrap();
    assert_eq!(n, 0);
    let reloaded = s.get_entry(&e.id).unwrap();
    assert_eq!(reloaded.current_version_id.as_deref(), Some(v1.id.as_str()));
    assert!(s.list_pending_reviews().unwrap().is_empty());

    let n = s.approve_version(&v2.id).unwrap();
    assert_eq!(n, 0); // 没有任何出现位置，自然没有范围
    assert_eq!(s.get_entry(&e.id).unwrap().current_version_id.as_deref(), Some(v2.id.as_str()));
}

#[test]
fn changing_exception_pronunciation_queues_only_role_occurrences() {
    let s = Studio::in_memory().unwrap();
    let (e, _) = s.create_entry(entry("可汗", "kè hán")).unwrap();
    s.upsert_exception(
        &e.id,
        NewException {
            scope_kind: "role".into(),
            scope_ref: "旁白".into(),
            ipa: "kě hán".into(),
            syllabification: String::new(),
            note: String::new(),
        },
    )
    .unwrap();
    let wav = make_wav(16000, 3.0, 0.5);
    let path = std::env::temp_dir().join("b.wav");
    std::fs::write(&path, &wav).unwrap();
    let rec = s
        .import_recording(NewRecording {
            chapter_no: 2,
            chapter_title: String::new(),
            item_no: "7".into(),
            src_path: path.to_string_lossy().into(),
            duration: None,
            copy_into_project: false,
        })
        .unwrap();
    s.add_occurrence(NewOccurrence {
        recording_id: rec.id.clone(),
        headword: "可汗".into(),
        chapter: Some(2),
        role: Some("旁白".into()),
        time_start: 0.2,
        time_end: 0.6,
    })
    .unwrap();
    s.add_occurrence(NewOccurrence {
        recording_id: rec.id.clone(),
        headword: "可汗".into(),
        chapter: Some(2),
        role: None,
        time_start: 1.2,
        time_end: 1.6,
    })
    .unwrap();

    let (_exc, n) = s
        .upsert_exception(
            &e.id,
            NewException {
                scope_kind: "role".into(),
                scope_ref: "旁白".into(),
                ipa: "kè hàn".into(),
                syllabification: String::new(),
                note: String::new(),
            },
        )
        .unwrap();
    assert_eq!(n, 1);
    let pending = s.list_pending_reviews().unwrap();
    assert_eq!(pending.len(), 1);
    assert!((pending[0].time_start - 0.2).abs() < 1e-9);
}
