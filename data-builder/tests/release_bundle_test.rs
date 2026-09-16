//! The release bundle is complete, self-verifying, byte-identical across builds, and refuses
//! to exist when the production state does not verify.

use data_builder_lib::production::verify_production_state;
use data_builder_lib::release::{
    build_release_bundle, build_release_bundle_from_state, classify_release, install_directory,
    install_directory_with, parse_c_abi_version, verify_release_bundle, RedistributionRecord,
    ReleaseOptions, C_HEADER_PATH,
};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

fn ws_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()
}

fn sha256(path: &Path) -> String {
    use sha2::Digest;
    format!("{:x}", sha2::Sha256::digest(fs::read(path).unwrap()))
}

fn files_under(dir: &Path) -> BTreeSet<String> {
    fn walk(dir: &Path, rel: &str, out: &mut BTreeSet<String>) {
        for e in fs::read_dir(dir).unwrap().flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            let child = if rel.is_empty() {
                name
            } else {
                format!("{}/{}", rel, name)
            };
            if e.path().is_dir() {
                walk(&e.path(), &child, out);
            } else {
                out.insert(child);
            }
        }
    }
    let mut out = BTreeSet::new();
    walk(dir, "", &mut out);
    out
}

fn copy_tree(src: &Path, dst: &Path) {
    if src.is_dir() {
        fs::create_dir_all(dst).unwrap();
        for e in fs::read_dir(src).unwrap().flatten() {
            copy_tree(&e.path(), &dst.join(e.file_name()));
        }
    } else {
        fs::copy(src, dst).unwrap();
    }
}

/// Rewrites `rel` (a JSON file) inside `bundle` through `mutate` and updates its SHA256SUMS
/// line, so the hash check passes and only the semantic checks are exercised.
fn rewrite_json(bundle: &Path, rel: &str, mutate: impl FnOnce(&mut serde_json::Value)) {
    let path = bundle.join(rel);
    let mut v: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    mutate(&mut v);
    fs::write(&path, serde_json::to_string_pretty(&v).unwrap() + "\n").unwrap();
    let sha = sha256(&path);
    let sums = fs::read_to_string(bundle.join("SHA256SUMS")).unwrap();
    let fixed: String = sums
        .lines()
        .map(|l| {
            if l.ends_with(&format!("  {}", rel)) {
                format!("{}  {}\n", sha, rel)
            } else {
                format!("{}\n", l)
            }
        })
        .collect();
    fs::write(bundle.join("SHA256SUMS"), fixed).unwrap();
}

#[test]
fn release_kind_is_evaluation_when_dirty_even_if_everything_is_allowed() {
    let allowed = vec![
        RedistributionRecord {
            subject: "source:manual-seed".to_string(),
            determination: "allowed".to_string(),
        },
        RedistributionRecord {
            subject: "language-model:x".to_string(),
            determination: "allowed".to_string(),
        },
    ];
    let clean = classify_release(&allowed, false);
    assert_eq!(clean.release_kind, "production");
    assert!(clean.evaluation_notice.is_none());

    let dirty = classify_release(&allowed, true);
    assert_eq!(dirty.release_kind, "evaluation");
    let notice = dirty.evaluation_notice.unwrap();
    assert!(
        notice.contains("uncommitted or untracked files"),
        "{}",
        notice
    );
    assert!(notice.contains("not reproducible"), "{}", notice);

    let mut pending = allowed.clone();
    pending[1].determination = "pending-review".to_string();
    let p = classify_release(&pending, false);
    assert_eq!(p.release_kind, "evaluation");
    let notice = p.evaluation_notice.unwrap();
    assert!(
        notice.contains("language-model:x = pending-review"),
        "{}",
        notice
    );
    assert!(!notice.contains("uncommitted"), "{}", notice);

    let both = classify_release(&pending, true);
    let notice = both.evaluation_notice.unwrap();
    assert!(
        notice.contains("pending-review") && notice.contains("uncommitted"),
        "{}",
        notice
    );
}

#[test]
fn installing_over_an_existing_bundle_never_loses_it() {
    let temp = tempfile::tempdir().unwrap();
    let stage = temp.path().join(".b.tmp-stage");
    let target = temp.path().join("b");
    let backup = temp.path().join(".b.tmp-backup");
    let make = |dir: &Path, content: &str| {
        let _ = fs::remove_dir_all(dir);
        fs::create_dir_all(dir.join("sub")).unwrap();
        fs::write(dir.join("sub/file.txt"), content).unwrap();
    };

    // First install: no previous bundle.
    make(&stage, "v1");
    install_directory(&stage, &target, &backup).unwrap();
    assert_eq!(
        fs::read_to_string(target.join("sub/file.txt")).unwrap(),
        "v1"
    );
    assert!(!stage.exists() && !backup.exists());

    // Replacement: the new bundle is installed, no backup is left behind.
    make(&stage, "v2");
    install_directory(&stage, &target, &backup).unwrap();
    assert_eq!(
        fs::read_to_string(target.join("sub/file.txt")).unwrap(),
        "v2"
    );
    assert!(!stage.exists() && !backup.exists());

    // A stale backup from an interrupted run is removed first.
    make(&backup, "stale");
    make(&stage, "v3");
    install_directory(&stage, &target, &backup).unwrap();
    assert_eq!(
        fs::read_to_string(target.join("sub/file.txt")).unwrap(),
        "v3"
    );
    assert!(!backup.exists());

    // Injected failure of the stage → target rename: the previous bundle is restored intact.
    make(&stage, "v4");
    let err = install_directory_with(&stage, &target, &backup, |from: &Path, to: &Path| {
        if from == stage {
            Err(std::io::Error::other("injected install failure"))
        } else {
            fs::rename(from, to)
        }
    })
    .unwrap_err();
    assert!(err.contains("injected install failure"), "{}", err);
    assert!(err.contains("previous bundle was restored"), "{}", err);
    assert_eq!(
        fs::read_to_string(target.join("sub/file.txt")).unwrap(),
        "v3"
    );
    assert!(
        !backup.exists(),
        "backup must be moved back, not left behind"
    );
    assert!(stage.exists(), "the failed stage is left for inspection");

    // Install and rollback both failing: nothing is deleted, the previous bundle survives as
    // the backup and the error says so.
    let err = install_directory_with(&stage, &target, &backup, |from: &Path, to: &Path| {
        if from == stage || from == backup {
            Err(std::io::Error::other("disk gone"))
        } else {
            fs::rename(from, to)
        }
    })
    .unwrap_err();
    assert!(err.contains("also failed"), "{}", err);
    assert_eq!(
        fs::read_to_string(backup.join("sub/file.txt")).unwrap(),
        "v3"
    );
    assert!(!target.exists());
    assert!(stage.exists());

    // The backup is now the only copy of the previous release. A later install whose stage
    // rename fails again must restore it first and leave it installed, never delete it.
    make(&stage, "v5");
    let err = install_directory_with(&stage, &target, &backup, |from: &Path, to: &Path| {
        if from == stage {
            Err(std::io::Error::other("still failing"))
        } else {
            fs::rename(from, to)
        }
    })
    .unwrap_err();
    assert!(err.contains("previous bundle was restored"), "{}", err);
    assert_eq!(
        fs::read_to_string(target.join("sub/file.txt")).unwrap(),
        "v3"
    );
    assert!(!backup.exists());

    // Back to the recovery state, then a restoration that fails: the backup stays intact and
    // nothing is installed.
    fs::rename(&target, &backup).unwrap();
    let err = install_directory_with(&stage, &target, &backup, |from: &Path, to: &Path| {
        if from == backup {
            Err(std::io::Error::other("cannot restore"))
        } else {
            fs::rename(from, to)
        }
    })
    .unwrap_err();
    assert!(
        err.contains("only as the backup") && err.contains("left intact"),
        "{}",
        err
    );
    assert_eq!(
        fs::read_to_string(backup.join("sub/file.txt")).unwrap(),
        "v3"
    );
    assert!(!target.exists());
    assert!(stage.exists());

    // Recovery state, then a healthy install: the previous release is restored, then
    // replaced by the new one; nothing is left behind.
    install_directory(&stage, &target, &backup).unwrap();
    assert_eq!(
        fs::read_to_string(target.join("sub/file.txt")).unwrap(),
        "v5"
    );
    assert!(!backup.exists() && !stage.exists());
}

#[test]
fn c_abi_version_is_read_from_the_header_only() {
    let header = fs::read_to_string(ws_root().join(C_HEADER_PATH)).unwrap();
    let v = parse_c_abi_version(&header).unwrap();
    assert_eq!(v.major, 1);
    assert!(v.minor >= 1);
    assert_eq!(
        parse_c_abi_version("#define KMR_ABI_VERSION_MAJOR 3U\n#define KMR_ABI_VERSION_MINOR 7U\n")
            .unwrap()
            .minor,
        7
    );
    assert!(parse_c_abi_version("#define KMR_ABI_VERSION_MAJOR 1U\n").is_err());
    assert!(parse_c_abi_version(
        "#define KMR_ABI_VERSION_MAJOR x\n#define KMR_ABI_VERSION_MINOR 1U\n"
    )
    .is_err());
}

#[test]
fn bundle_refuses_when_production_state_fails_and_writes_nothing() {
    let root = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    let err =
        build_release_bundle(root.path(), &ReleaseOptions::default(), out.path()).unwrap_err();
    assert!(err.contains("production state is not OK"), "{}", err);
    assert!(err.contains("nothing was written"), "{}", err);
    assert!(fs::read_dir(out.path()).unwrap().next().is_none());
}

#[test]
fn workspace_bundle_is_complete_self_verifying_byte_identical_and_tamper_evident() {
    let root = ws_root();
    let state = verify_production_state(root).unwrap();
    assert!(state.ok);
    let options = ReleaseOptions {
        allow_dirty: true,
        ..Default::default()
    };

    // Two builds are byte-identical in every file.
    let out_a = tempfile::tempdir().unwrap();
    let out_b = tempfile::tempdir().unwrap();
    let a = build_release_bundle_from_state(root, &state, &options, out_a.path()).unwrap();
    let b = build_release_bundle_from_state(root, &state, &options, out_b.path()).unwrap();
    assert_eq!(a.sha256sums_sha256, b.sha256sums_sha256);
    let dir_a = PathBuf::from(&a.bundle_dir);
    let dir_b = PathBuf::from(&b.bundle_dir);
    assert_eq!(files_under(&dir_a), files_under(&dir_b));
    for rel in files_under(&dir_a) {
        assert_eq!(
            fs::read(dir_a.join(&rel)).unwrap(),
            fs::read(dir_b.join(&rel)).unwrap(),
            "{}",
            rel
        );
    }

    // Layout and coverage: every file is listed in SHA256SUMS, nothing else exists.
    let present = files_under(&dir_a);
    for expected in [
        "VERSION",
        "SHA256SUMS",
        "provenance.json",
        "compatibility.json",
        "ATTRIBUTION",
        "include/kurmanci.h",
        "LICENSES/LICENSE",
        "LICENSES/NOTICE",
        "LICENSES/manual-seed/LICENSE",
        "packs/seed/lexicon.bin",
        "packs/reviewed/lexicon.bin",
        "packs/reviewed/manifest.json",
        "packs/reviewed/artifacts.sha256",
        "packs/experimental-full/lexicon.bin",
        "language-model/kuwiki-20260801/manifest.json",
        "language-model/kuwiki-20260801/vocabulary.txt",
    ] {
        assert!(present.contains(expected), "missing {}", expected);
    }
    assert_eq!(present.len(), a.file_count);
    let sums = fs::read_to_string(dir_a.join("SHA256SUMS")).unwrap();
    let listed: BTreeSet<String> = sums
        .lines()
        .map(|l| l.split_once("  ").unwrap().1.to_string())
        .collect();
    let mut expected_listed = present.clone();
    expected_listed.remove("SHA256SUMS");
    assert_eq!(listed, expected_listed);
    assert_eq!(sha256(&dir_a.join("SHA256SUMS")), a.sha256sums_sha256);
    assert_eq!(
        fs::read_to_string(dir_a.join("VERSION")).unwrap(),
        format!("{}\n", kurmanci_engine::compat::ENGINE_VERSION)
    );
    assert_eq!(
        dir_a.file_name().unwrap().to_string_lossy(),
        format!(
            "kurmanci-ku-Latn-{}",
            kurmanci_engine::compat::ENGINE_VERSION
        )
    );

    // Provenance: hashes equal the verified build artifacts; licensing state is visible.
    let p = &a.provenance;
    assert_eq!(p.packs.len(), 3);
    for pack in &p.packs {
        for name in ["lexicon.bin", "manifest.json", "attribution.txt"] {
            assert_eq!(
                pack.files[name],
                sha256(&root.join(format!("data/build/packs/{}/{}", pack.pack_id, name))),
                "{}/{}",
                pack.pack_id,
                name
            );
        }
    }
    let reviewed = p.packs.iter().find(|x| x.pack_id == "reviewed").unwrap();
    assert_eq!(
        reviewed.language_model_id.as_deref(),
        Some("kuwiki-20260801")
    );
    assert!(!reviewed.source_provenance.is_empty());
    let lm = p.language_model.as_ref().unwrap();
    assert_eq!(lm.model_id, "kuwiki-20260801");
    assert_eq!(lm.licensing.redistribution_determination, "pending-review");
    assert_eq!(lm.files.len(), 6);
    assert_eq!(p.release_kind, "evaluation");
    let notice = p.evaluation_notice.as_deref().unwrap();
    assert!(
        notice.contains("language-model:kuwiki-20260801 = pending-review"),
        "{}",
        notice
    );
    assert_eq!(
        notice.contains("uncommitted or untracked files"),
        p.source.worktree_dirty,
        "{}",
        notice
    );
    assert!(p
        .licensing
        .spdx_identifiers
        .contains(&"CC-BY-SA-4.0".to_string()));
    assert!(p
        .licensing
        .spdx_identifiers
        .contains(&"Apache-2.0".to_string()));
    assert!(p
        .sources
        .iter()
        .any(|s| s.source_id == "kurdish-hunspell-kmr"));
    assert!(p
        .corpora
        .iter()
        .any(|c| c.corpus_id == "kuwiki" && c.source_artifact_sha256.is_some()));
    assert_eq!(p.source.commit.len(), 40);
    assert_eq!(p.source.data_tree.len(), 40);
    assert_eq!(p.toolchain.rust, "1.85.0");
    assert_eq!(p.engine_version, kurmanci_engine::compat::ENGINE_VERSION);
    assert_eq!(p.language_tag, "ku-Latn");
    assert_eq!(
        p.pack_schema_version,
        kurmanci_engine::compat::PACK_SCHEMA_VERSION
    );
    let header = fs::read_to_string(root.join(C_HEADER_PATH)).unwrap();
    assert_eq!(p.c_abi_version, parse_c_abi_version(&header).unwrap());
    assert!(p.production_state.ok);
    let compat: serde_json::Value =
        serde_json::from_slice(&fs::read(dir_a.join("compatibility.json")).unwrap()).unwrap();
    assert_eq!(
        compat["engine_version"],
        kurmanci_engine::compat::ENGINE_VERSION
    );
    assert_eq!(compat["supported_pack_schemas"], serde_json::json!([4]));
    assert_eq!(compat["language_tag"], "ku-Latn");
    assert_eq!(compat["c_abi_version"]["major"], 1);
    let attribution = fs::read_to_string(dir_a.join("ATTRIBUTION")).unwrap();
    assert!(attribution.contains("# Pack reviewed"));

    // Self-verification passes on the pristine bundle.
    let v = verify_release_bundle(&dir_a).unwrap();
    assert_eq!(v.file_count, a.file_count);
    assert_eq!(v.sha256sums_sha256, a.sha256sums_sha256);
    assert_eq!(v.provenance, a.provenance);

    // Tampering is detected: a flipped byte, an extra file, a missing file.
    let tampered = tempfile::tempdir().unwrap();
    let t = tampered.path().join("bundle");
    copy_tree(&dir_a, &t);
    let lexicon = t.join("packs/seed/lexicon.bin");
    let mut bytes = fs::read(&lexicon).unwrap();
    let mid = bytes.len() / 2;
    bytes[mid] ^= 0xFF;
    fs::write(&lexicon, &bytes).unwrap();
    let err = verify_release_bundle(&t).unwrap_err();
    assert!(err.contains("packs/seed/lexicon.bin"), "{}", err);
    copy_tree(&dir_a.join("packs/seed/lexicon.bin"), &lexicon);
    verify_release_bundle(&t).unwrap();
    fs::write(
        t.join("apple/extra.txt")
            .parent()
            .map(|p| {
                fs::create_dir_all(p).unwrap();
                p.join("extra.txt")
            })
            .unwrap(),
        b"x",
    )
    .unwrap();
    let err = verify_release_bundle(&t).unwrap_err();
    assert!(err.contains("not listed in SHA256SUMS"), "{}", err);
    fs::remove_dir_all(t.join("apple")).unwrap();
    fs::remove_file(t.join("ATTRIBUTION")).unwrap();
    let err = verify_release_bundle(&t).unwrap_err();
    assert!(
        err.contains("ATTRIBUTION") && err.contains("missing"),
        "{}",
        err
    );

    // Symbolic links are never followed: an artifact tree escaping through a link fails
    // naming the link, and nothing is written; a link inside a bundle fails verification.
    #[cfg(unix)]
    {
        let outside = tempfile::tempdir().unwrap();
        let secret = outside.path().join("outside-secret.txt");
        fs::write(&secret, b"must never be read").unwrap();
        let tree = tempfile::tempdir().unwrap();
        let framework = tree.path().join("Escaping.xcframework");
        fs::create_dir_all(framework.join("ios")).unwrap();
        fs::write(framework.join("ios/libkurmanci.a"), b"lib").unwrap();
        std::os::unix::fs::symlink(&secret, framework.join("ios/escape")).unwrap();
        let dangling = tree.path().join("Dangling.xcframework");
        fs::create_dir_all(&dangling).unwrap();
        std::os::unix::fs::symlink("/nonexistent/target", dangling.join("gone")).unwrap();
        for artifact in [framework.clone(), dangling.clone()] {
            let out = tempfile::tempdir().unwrap();
            let opts = ReleaseOptions {
                allow_dirty: true,
                apple_artifacts: vec![artifact.clone()],
                ..Default::default()
            };
            let err = build_release_bundle_from_state(root, &state, &opts, out.path()).unwrap_err();
            assert!(err.contains("symbolic link"), "{}", err);
            assert!(
                err.contains("escape") || err.contains("gone"),
                "error must name the link: {}",
                err
            );
            assert!(!err.contains("must never be read"));
            assert!(
                fs::read_dir(out.path()).unwrap().next().is_none(),
                "nothing written"
            );
        }
        // A symlink passed directly as the artifact is rejected too.
        let link = tree.path().join("link-to-zip");
        std::os::unix::fs::symlink(&secret, &link).unwrap();
        let out = tempfile::tempdir().unwrap();
        let opts = ReleaseOptions {
            allow_dirty: true,
            android_artifacts: vec![link.clone()],
            ..Default::default()
        };
        let err = build_release_bundle_from_state(root, &state, &opts, out.path()).unwrap_err();
        assert!(
            err.contains("symbolic link") && err.contains("link-to-zip"),
            "{}",
            err
        );

        let linked = tempfile::tempdir().unwrap();
        let lb = linked.path().join("bundle");
        copy_tree(&dir_a, &lb);
        std::os::unix::fs::symlink(&secret, lb.join("LICENSES/escape")).unwrap();
        let err = verify_release_bundle(&lb).unwrap_err();
        assert!(
            err.contains("symbolic link") && err.contains("escape"),
            "{}",
            err
        );
        let root_link = linked.path().join("bundle-link");
        std::os::unix::fs::symlink(&lb, &root_link).unwrap();
        let err = verify_release_bundle(&root_link).unwrap_err();
        assert!(err.contains("symbolic link"), "{}", err);
    }

    // The verifier enforces the release-kind rules and the LM schema agreement on its own.
    let semantic = tempfile::tempdir().unwrap();
    let sb = semantic.path().join("bundle");
    copy_tree(&dir_a, &sb);
    verify_release_bundle(&sb).unwrap();
    rewrite_json(&sb, "provenance.json", |v| {
        v["source"]["worktree_dirty"] = serde_json::json!(true);
        v["release_kind"] = serde_json::json!("production");
    });
    let err = verify_release_bundle(&sb).unwrap_err();
    assert!(
        err.contains("worktree_dirty") && err.contains("production"),
        "{}",
        err
    );
    copy_tree(&dir_a, &sb);
    rewrite_json(&sb, "provenance.json", |v| {
        // Clean tree claimed, but the language model is pending-review: not production.
        v["source"]["worktree_dirty"] = serde_json::json!(false);
        v["release_kind"] = serde_json::json!("production");
    });
    let err = verify_release_bundle(&sb).unwrap_err();
    assert!(err.contains("does not follow"), "{}", err);
    copy_tree(&dir_a, &sb);
    rewrite_json(&sb, "provenance.json", |v| {
        v.as_object_mut().unwrap().remove("evaluation_notice");
    });
    let err = verify_release_bundle(&sb).unwrap_err();
    assert!(err.contains("evaluation notice"), "{}", err);
    copy_tree(&dir_a, &sb);
    rewrite_json(&sb, "compatibility.json", |v| {
        v["language_model_schema_version"] = serde_json::json!(99);
    });
    let err = verify_release_bundle(&sb).unwrap_err();
    assert!(err.contains("language_model_schema_version"), "{}", err);
    copy_tree(&dir_a, &sb);
    verify_release_bundle(&sb).unwrap();

    // An untracked file under a bundle input directory is a dirty tree: refused without
    // --allow-dirty, and with it recorded as dirty and classified as an evaluation release.
    {
        struct Guard(PathBuf);
        impl Drop for Guard {
            fn drop(&mut self) {
                let _ = fs::remove_file(&self.0);
            }
        }
        let untracked = root.join("data/licenses/zz-untracked-test-input.txt");
        assert!(!untracked.exists());
        fs::write(&untracked, b"untracked bundle input").unwrap();
        let _guard = Guard(untracked.clone());
        let out = tempfile::tempdir().unwrap();
        let strict = ReleaseOptions::default();
        let err = build_release_bundle_from_state(root, &state, &strict, out.path()).unwrap_err();
        assert!(err.contains("untracked"), "{}", err);
        assert!(err.contains("nothing was written"), "{}", err);
        assert!(fs::read_dir(out.path()).unwrap().next().is_none());
        let lenient = ReleaseOptions {
            allow_dirty: true,
            ..Default::default()
        };
        let d = build_release_bundle_from_state(root, &state, &lenient, out.path()).unwrap();
        assert!(d.provenance.source.worktree_dirty);
        assert_eq!(d.provenance.release_kind, "evaluation");
        let notice = d.provenance.evaluation_notice.as_deref().unwrap();
        assert!(notice.contains("untracked"), "{}", notice);
        assert!(
            notice.contains("not reproducible from the recorded source commit and data tree"),
            "{}",
            notice
        );
        assert!(files_under(&PathBuf::from(&d.bundle_dir))
            .contains("LICENSES/zz-untracked-test-input.txt"));
        verify_release_bundle(&d.bundle_dir).unwrap();
    }

    // Replacing an installed bundle in place keeps a single, verifiable bundle.
    let replaced = build_release_bundle_from_state(root, &state, &options, out_a.path()).unwrap();
    assert_eq!(replaced.sha256sums_sha256, a.sha256sums_sha256);
    assert!(!out_a
        .path()
        .join(".kurmanci-ku-Latn-0.1.0.tmp-backup")
        .exists());
    assert!(!out_a
        .path()
        .join(".kurmanci-ku-Latn-0.1.0.tmp-stage")
        .exists());
    verify_release_bundle(&dir_a).unwrap();

    // Platform artifacts are attached, hashed, listed and verified.
    let artifacts = tempfile::tempdir().unwrap();
    let zip = artifacts.path().join("KurmanciFFI-v0.1.0.xcframework.zip");
    fs::write(&zip, b"not a real xcframework, but hashed as received").unwrap();
    let aar_dir = artifacts.path().join("maven");
    fs::create_dir_all(aar_dir.join("org/kurmanci")).unwrap();
    fs::write(
        aar_dir.join("org/kurmanci/kurmanci-android-0.1.0.aar"),
        b"aar bytes",
    )
    .unwrap();
    let with_platform = ReleaseOptions {
        allow_dirty: true,
        release_version: Some("0.1.0-eval.1".to_string()),
        apple_artifacts: vec![zip.clone()],
        android_artifacts: vec![aar_dir.clone()],
    };
    let out_c = tempfile::tempdir().unwrap();
    let c = build_release_bundle_from_state(root, &state, &with_platform, out_c.path()).unwrap();
    let dir_c = PathBuf::from(&c.bundle_dir);
    assert!(dir_c.ends_with("kurmanci-ku-Latn-0.1.0-eval.1"));
    assert_eq!(c.provenance.platform_artifacts.len(), 2);
    let paths: Vec<&str> = c
        .provenance
        .platform_artifacts
        .iter()
        .map(|a| a.path.as_str())
        .collect();
    assert_eq!(
        paths,
        vec![
            "apple/KurmanciFFI-v0.1.0.xcframework.zip",
            "android/maven/org/kurmanci/kurmanci-android-0.1.0.aar"
        ]
    );
    assert_eq!(c.provenance.platform_artifacts[0].sha256, sha256(&zip));
    assert!(fs::read_to_string(dir_c.join("SHA256SUMS"))
        .unwrap()
        .contains("  apple/KurmanciFFI-v0.1.0.xcframework.zip"));
    verify_release_bundle(&dir_c).unwrap();
    assert_ne!(c.sha256sums_sha256, a.sha256sums_sha256);
}
