//! 多审听者合并：导出离线包、互相导入、幂等、冲突与越界拦截、差异报告。

use ab_core::diff::{diff_snapshots, load_snapshot, render_text_report};
use ab_core::models::*;
use ab_core::{wav, Studio};

fn seed_project(name: &str) -> (Studio, String, String) {
    let dir = std::env::temp_dir().join(format!("ab-{}-{}", name, uuid_like()));
    let s = Studio::open(&dir).unwrap();
    s.set_project_name(name).unwrap();
    let entry = s
        .create_entry(NewEntry {
            headword: "月氏".into(),
            scope: "全书".into(),
            kind: "place".into(),
            tags: String::new(),
            ipa: "yuè zhī".into(),
            syllabification: String::new(),
            example_audio: None,
            source: "作者确认".into(),
            approved: true,
            note: String::new(),
        })
        .unwrap()
        .0;
    let src = std::env::temp_dir().join(format!("{}-{}.wav", name, uuid_like()));
    std::fs::write(&src, wav::make_wav(16000, 6.0, 0.5)).unwrap();
    let rec = s
        .import_recording(NewRecording {
            chapter_no: 1,
            chapter_title: "第一章".into(),
            item_no: "1".into(),
            src_path: src.to_string_lossy().into(),
            duration: None,
            copy_into_project: true,
        })
        .unwrap();
    s.add_occurrence(NewOccurrence {
        recording_id: rec.id.clone(),
        headword: "月氏".into(),
        chapter: Some(1),
        role: None,
        time_start: 0.5,
        time_end: 1.0,
    })
    .unwrap();
    (s, entry.id, rec.id)
}

fn uuid_like() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let n = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    format!("{n:x}")
}

#[test]
fn two_reviewers_packages_merge_idempotently() {
    // 主编工程（base）
    let (base, entry_id, rec_id) = seed_project("原著");
    let before = base.snapshot(&PackageOptions { include_media: true, reviewer_filter: None }).unwrap();

    // 审听者甲：带包离线审听
    let dir_a = std::env::temp_dir().join(format!("ab-a-{}", uuid_like()));
    let a = Studio::open(&dir_a).unwrap();
    let pkg_base = std::env::temp_dir().join("base.abpkg");
    base.export_package(&pkg_base, &PackageOptions { include_media: true, reviewer_filter: None }, Some("主编"))
        .unwrap();
    let rep = a.import_package(&pkg_base).unwrap();
    assert!(rep.packages_applied[0].contains("原著"));
    assert_eq!(rep.recordings_added, 1);

    // 甲在 1.0s 处批注误读，并导出自己的包（只含甲的批注）
    a.add_annotation(NewAnnotation {
        recording_id: rec_id.clone(),
        kind: AnnotationKind::Mispron,
        time_start: 1.0,
        time_end: 1.3,
        comment: "读成了 ròu zhī".into(),
        entry_version_id: Some(a.list_versions(&entry_id).unwrap()[0].id.clone()),
        reviewer: Some("审听甲".into()),
    })
    .unwrap();
    let pkg_a = std::env::temp_dir().join("reviewer-a.abpkg");
    a.export_package(
        &pkg_a,
        &PackageOptions { include_media: false, reviewer_filter: Some("审听甲".into()) },
        Some("审听甲"),
    )
    .unwrap();

    // 审听者乙：从 base 包起步，标一条噪声
    let dir_b = std::env::temp_dir().join(format!("ab-b-{}", uuid_like()));
    let b = Studio::open(&dir_b).unwrap();
    b.import_package(&pkg_base).unwrap();
    b.add_annotation(NewAnnotation {
        recording_id: rec_id.clone(),
        kind: AnnotationKind::Noise,
        time_start: 3.0,
        time_end: 3.6,
        comment: "空调噪声".into(),
        entry_version_id: None,
        reviewer: Some("审听乙".into()),
    })
    .unwrap();
    let pkg_b = std::env::temp_dir().join("reviewer-b.abpkg");
    b.export_package(
        &pkg_b,
        &PackageOptions { include_media: false, reviewer_filter: Some("审听乙".into()) },
        Some("审听乙"),
    )
    .unwrap();

    // 主编依次合并甲、乙
    let rep_a = base.import_package(&pkg_a).unwrap();
    assert_eq!(rep_a.annotations_added, 1);
    assert_eq!(rep_a.review_ranges_added, 1, "外来未决批注生成待复核");
    let rep_b = base.import_package(&pkg_b).unwrap();
    assert_eq!(rep_b.annotations_added, 1);

    // 幂等：重复导入甲的包不产生重复
    let rep_a2 = base.import_package(&pkg_a).unwrap();
    assert_eq!(rep_a2.annotations_added, 0);
    assert_eq!(rep_a2.annotations_conflicted, 0);
    assert!(rep_a2.skipped.is_empty());

    let anns = base.list_annotations(&rec_id).unwrap();
    assert_eq!(anns.len(), 2);
    let reviewers: std::collections::BTreeSet<_> = anns.iter().map(|a| a.reviewer.clone()).collect();
    assert!(reviewers.contains("审听甲") && reviewers.contains("审听乙"));

    // 待复核：两条外来批注各一条（不同时间段）
    assert_eq!(base.list_pending_reviews().unwrap().len(), 2);

    // base 包自身可重复导入（幂等，不新增录音）
    let rep_base_again = base.import_package(&pkg_base).unwrap();
    assert_eq!(rep_base_again.recordings_added, 0);

    // 差异报告：before -> after
    let after = base.snapshot(&PackageOptions { include_media: false, reviewer_filter: None }).unwrap();
    let d = diff_snapshots(&before, &after);
    assert_eq!(d.annotations_added.len(), 2);
    let text = render_text_report(&d);
    assert!(text.contains("差异报告"));
    assert!(text.contains("审听甲"));
    assert!(text.contains("审听乙"));
    println!("\n{text}");
}

#[test]
fn out_of_bounds_annotation_in_package_is_conflict_not_crash() {
    let (base, _entry, rec_id) = seed_project("主编2");
    // 乙的工程里，录音被换成 1s 的短文件，乙却保留了 7s 的批注
    // （超过双方工程时长，主编合并时必须拦截）
    let dir = std::env::temp_dir().join(format!("ab-c-{}", uuid_like()));
    let c = Studio::open(&dir).unwrap();
    let pkg_base = std::env::temp_dir().join("base2.abpkg");
    base.export_package(&pkg_base, &PackageOptions { include_media: true, reviewer_filter: None }, None)
        .unwrap();
    c.import_package(&pkg_base).unwrap();

    // 直接把本地录音替成 1s 并更新摘要
    let rec = c.get_recording(&rec_id).unwrap();
    std::fs::write(&rec.file_path, wav::make_wav(16000, 1.0, 0.5)).unwrap();
    let v = c.verify_recording(&rec_id).unwrap();
    assert!(v.changed);

    // 手工塞一条越界批注到 c 的数据库（绕过严格校验，模拟“别人的旧工程”）
    c.conn
        .execute(
            "INSERT INTO annotation(id, recording_id, kind, time_start, time_end, comment,
                                    entry_version_id, expected_ipa, reviewer, created_at, resolved)
             VALUES('ann-oob', ?1, 'noise', 7.0, 7.5, '越界噪声', NULL, NULL, '审听丙',
                    datetime('now'), 0)",
            [&rec_id],
        )
        .unwrap();
    let pkg_c = std::env::temp_dir().join("reviewer-c.abpkg");
    c.export_package(
        &pkg_c,
        &PackageOptions { include_media: false, reviewer_filter: Some("审听丙".into()) },
        Some("审听丙"),
    )
    .unwrap();

    let rep = base.import_package(&pkg_c).unwrap();
    assert_eq!(rep.annotations_added, 0);
    assert_eq!(rep.annotations_conflicted, 1);
    let kinds: Vec<_> = rep.conflicts.iter().map(|c| c.kind.as_str()).collect();
    assert!(kinds.contains(&"annotation_out_of_bounds"), "冲突: {kinds:?}");
    assert!(base.list_annotations(&rec_id).unwrap().is_empty());
}

#[test]
fn divergent_recording_and_exception_are_reported_as_conflicts() {
    let (base, entry_id, rec_id) = seed_project("主编3");
    let pkg_base = std::env::temp_dir().join("base3.abpkg");
    base.export_package(&pkg_base, &PackageOptions { include_media: true, reviewer_filter: None }, None)
        .unwrap();

    let dir = std::env::temp_dir().join(format!("ab-d-{}", uuid_like()));
    let d = Studio::open(&dir).unwrap();
    d.import_package(&pkg_base).unwrap();

    // 1) 相同录音 id，不同内容
    let rec = d.get_recording(&rec_id).unwrap();
    std::fs::write(&rec.file_path, wav::make_wav(16000, 6.0, 0.9)).unwrap();
    let _ = d.verify_recording(&rec_id).unwrap();

    // 2) 相同 (条目, 角色) 例外，不同读音
    d.upsert_exception(
        &entry_id,
        NewException {
            scope_kind: "role".into(),
            scope_ref: "说书人".into(),
            ipa: "ròu zhī".into(),
            syllabification: String::new(),
            note: String::new(),
        },
    )
    .unwrap();

    let pkg_d = std::env::temp_dir().join("reviewer-d.abpkg");
    d.export_package(&pkg_d, &PackageOptions { include_media: true, reviewer_filter: None }, None)
        .unwrap();

    // 主编先加同键但不同读音的例外
    base.upsert_exception(
        &entry_id,
        NewException {
            scope_kind: "role".into(),
            scope_ref: "说书人".into(),
            ipa: "yuè zhì".into(),
            syllabification: String::new(),
            note: String::new(),
        },
    )
    .unwrap();

    let rep = base.import_package(&pkg_d).unwrap();
    let kinds: Vec<_> = rep.conflicts.iter().map(|c| c.kind.as_str()).collect();
    assert!(kinds.contains(&"recording_divergent"), "冲突: {kinds:?}");
    assert!(kinds.contains(&"exception_divergent"), "冲突: {kinds:?}");
    // 本地读音不被覆盖
    let excs = base.list_exceptions(&entry_id).unwrap();
    let local = excs.iter().find(|e| e.scope_ref == "说书人").unwrap();
    assert_eq!(local.ipa, "yuè zhì");
}

#[test]
fn package_roundtrip_loads_as_snapshot_and_zip_is_valid() {
    let (s, _e, _r) = seed_project("主编4");
    let pkg = std::env::temp_dir().join("round.abpkg");
    s.export_package(&pkg, &PackageOptions { include_media: true, reviewer_filter: None }, None)
        .unwrap();
    let bytes = std::fs::read(&pkg).unwrap();
    assert_eq!(&bytes[..2], b"PK");
    let snap = load_snapshot(&bytes).unwrap();
    assert_eq!(snap.project, "主编4");
    assert_eq!(snap.recordings.len(), 1);
    assert_eq!(snap.media_count, 1);
}
