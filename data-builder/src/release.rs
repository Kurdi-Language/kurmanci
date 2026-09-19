//! Deterministic release bundle: one directory a vendor can evaluate.
//!
//! `build_release_bundle` refuses unless `verify_production_state` passes, then assembles
//! `kurmanci-<language-tag>-<version>/` entirely in memory and installs it by failure-safe
//! staged replacement (stage, park the previous bundle as a backup, rename, restore on failure):
//!
//! ```text
//! kurmanci-ku-Latn-X.Y.Z/
//! ├── VERSION
//! ├── compatibility.json      machine-readable compatibility table (engine, C ABI, schemas)
//! ├── provenance.json         everything the release is made of, with hashes
//! ├── SHA256SUMS              every other file in the bundle
//! ├── ATTRIBUTION             the attribution text of every pack
//! ├── LICENSES/               repository licence and notice, per-source licence files
//! ├── include/kurmanci.h      the C ABI header
//! ├── packs/<pack-id>/        the five verified artifacts of every pack
//! ├── language-model/<id>/    the committed, non-prose language model artifacts
//! ├── apple/                  optional: artifacts produced by scripts/apple
//! └── android/                optional: artifacts produced by scripts/android
//! ```
//!
//! Nothing in the bundle carries a timestamp, so two clean checkouts of the same commit
//! produce byte-identical bundles (`scripts/release/verify-clean-checkout-determinism.sh`).
//! Platform artifacts are attached and hashed but their reproducibility is not asserted here;
//! they are built by the existing Apple and Android scripts.
//!
//! No legal determination is made: every redistribution determination is copied from the
//! registries and the language model as recorded (`pending-review` stays visible), and the
//! bundle is labelled an evaluation release unless every determination is `allowed`.
//! `verify_release_bundle` re-checks a bundle directory read-only.

use crate::corpus::registry::CorpusRegistry;
use crate::pack::builder::PACK_ARTIFACT_FILES;
use crate::pack::language_model::{
    load_language_model, LanguageModelLicensing, LANGUAGE_MODEL_DIR,
};
use crate::pack::manifest::{DataLicenseEntry, PackManifest, SourceReviewProvenance};
use crate::pack::policy::PackPolicyConfig;
use crate::production::{render_state_text, verify_production_state, ProductionStateReport};
use crate::sources::SourceRegistry;
use kurmanci_engine::compat::{CompatibilityTable, ENGINE_VERSION, SUPPORTED_LANGUAGE_TAG};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const RELEASE_BUNDLE_LAYOUT_VERSION: &str = "release-bundle-v1";
pub const RELEASE_PROVENANCE_SCHEMA_VERSION: &str = "release-provenance-v1";
pub const RELEASE_COMPATIBILITY_SCHEMA_VERSION: &str = "release-compatibility-v1";
pub const SOURCE_REPOSITORY: &str = "Kurdi-Language/kurmanci";
pub const C_HEADER_PATH: &str = "ffi/include/kurmanci.h";
const LANGUAGE_MODEL_FILES: [&str; 6] = [
    "vocabulary.txt",
    "unigrams.tsv",
    "bigrams.tsv",
    "trigrams.tsv",
    "manifest.json",
    "artifacts.sha256",
];
const REDISTRIBUTION_ALLOWED: &str = "allowed";

#[derive(Debug, Clone, Default)]
pub struct ReleaseOptions {
    /// Release version string; defaults to the engine crate version.
    pub release_version: Option<String>,
    /// Build even when tracked files have uncommitted changes (recorded in the provenance).
    pub allow_dirty: bool,
    /// Files or directories produced by `scripts/apple`, attached under `apple/`.
    pub apple_artifacts: Vec<PathBuf>,
    /// Files or directories produced by `scripts/android`, attached under `android/`.
    pub android_artifacts: Vec<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CAbiVersion {
    pub major: u32,
    pub minor: u32,
}

/// Reads `KMR_ABI_VERSION_MAJOR` / `KMR_ABI_VERSION_MINOR` from the C header text, the one
/// place the C ABI version is defined.
pub fn parse_c_abi_version(header: &str) -> Result<CAbiVersion, String> {
    fn define(header: &str, name: &str) -> Result<u32, String> {
        for line in header.lines() {
            if let Some(rest) = line.trim().strip_prefix("#define ") {
                let mut parts = rest.split_whitespace();
                if parts.next() == Some(name) {
                    let value = parts
                        .next()
                        .ok_or_else(|| format!("{} has no value in kurmanci.h", name))?;
                    let digits: String = value.chars().take_while(|c| c.is_ascii_digit()).collect();
                    return digits
                        .parse()
                        .map_err(|_| format!("{} is not numeric in kurmanci.h: {}", name, value));
                }
            }
        }
        Err(format!("{} not found in kurmanci.h", name))
    }
    Ok(CAbiVersion {
        major: define(header, "KMR_ABI_VERSION_MAJOR")?,
        minor: define(header, "KMR_ABI_VERSION_MINOR")?,
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompatibilityManifest {
    pub schema_version: String,
    pub engine_version: String,
    pub c_abi_version: CAbiVersion,
    pub pack_magic: String,
    pub pack_schema_version: u32,
    pub supported_pack_schemas: Vec<u32>,
    pub language_model_schema_version: u32,
    pub supported_language_model_schemas: Vec<u32>,
    pub language_tag: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseClassification {
    pub release_kind: String,
    pub evaluation_notice: Option<String>,
}

/// `production` only when every redistribution determination is `allowed` *and* the tracked
/// tree was clean; anything else is an `evaluation` release with a notice naming why. The
/// determinations are restated as recorded; this is not a legal determination.
pub fn classify_release(
    redistribution: &[RedistributionRecord],
    worktree_dirty: bool,
) -> ReleaseClassification {
    let pending: Vec<String> = redistribution
        .iter()
        .filter(|r| r.determination != REDISTRIBUTION_ALLOWED)
        .map(|r| format!("{} = {}", r.subject, r.determination))
        .collect();
    let mut reasons = Vec::new();
    if !pending.is_empty() {
        reasons.push(format!(
            "redistribution is not determined to be allowed for: {} (determinations are copied from the registries and the language model as recorded; this tool makes no legal determination; do not redistribute until they are resolved)",
            pending.join(", ")
        ));
    }
    if worktree_dirty {
        reasons.push(
            "the working tree had uncommitted or untracked files when the bundle was built (worktree_dirty = true), so the bundle is not reproducible from the recorded source commit and data tree"
                .to_string(),
        );
    }
    if reasons.is_empty() {
        ReleaseClassification {
            release_kind: "production".to_string(),
            evaluation_notice: None,
        }
    } else {
        ReleaseClassification {
            release_kind: "evaluation".to_string(),
            evaluation_notice: Some(format!(
                "Evaluation/development release: {}.",
                reasons.join("; ")
            )),
        }
    }
}

/// Failure-safe staged replacement of `target` by the complete directory `stage`: a previous
/// `target` is parked at `backup`, `stage` is renamed into place, and on failure the previous
/// `target` is restored. The previous complete release is never deleted before the new one
/// is installed, including when a prior run left it only in `backup`. This is not a no-gap
/// atomic exchange: between the two renames `target` is briefly absent. `rename` is the
/// primitive, injectable for tests.
pub fn install_directory_with<F>(
    stage: &Path,
    target: &Path,
    backup: &Path,
    mut rename: F,
) -> Result<(), String>
where
    F: FnMut(&Path, &Path) -> std::io::Result<()>,
{
    let target_exists = fs::symlink_metadata(target).is_ok();
    let backup_exists = fs::symlink_metadata(backup).is_ok();
    match (target_exists, backup_exists) {
        // Target is authoritative; a leftover backup from an interrupted run is stale.
        (true, true) => {
            fs::remove_dir_all(backup)
                .map_err(|e| format!("Failed to remove stale backup {:?}: {}", backup, e))?;
        }
        // Only the backup holds the previous complete release (a failed install whose
        // rollback also failed): it is recovery-critical. Restore it first; if that fails,
        // leave it untouched and stop.
        (false, true) => {
            rename(backup, target).map_err(|e| {
                format!(
                    "the previous bundle exists only as the backup {:?} and could not be restored to {:?}: {} (backup left intact; nothing was installed)",
                    backup, target, e
                )
            })?;
        }
        (true, false) | (false, false) => {}
    }
    let had_previous = target_exists || backup_exists;
    if had_previous {
        rename(target, backup).map_err(|e| {
            format!(
                "Failed to park the previous bundle {:?} at {:?}: {} (nothing was replaced)",
                target, backup, e
            )
        })?;
    }
    match rename(stage, target) {
        Ok(()) => {
            if had_previous {
                fs::remove_dir_all(backup).map_err(|e| {
                    format!(
                        "bundle installed at {:?}, but the previous bundle could not be removed from {:?}: {}",
                        target, backup, e
                    )
                })?;
            }
            Ok(())
        }
        Err(install_err) => {
            if had_previous {
                match rename(backup, target) {
                    Ok(()) => Err(format!(
                        "Failed to install bundle {:?}: {}; the previous bundle was restored",
                        target, install_err
                    )),
                    Err(rollback_err) => Err(format!(
                        "Failed to install bundle {:?}: {}; restoring the previous bundle from {:?} also failed: {}",
                        target, install_err, backup, rollback_err
                    )),
                }
            } else {
                Err(format!(
                    "Failed to install bundle {:?}: {}",
                    target, install_err
                ))
            }
        }
    }
}

pub fn install_directory(stage: &Path, target: &Path, backup: &Path) -> Result<(), String> {
    install_directory_with(stage, target, backup, |from: &Path, to: &Path| {
        fs::rename(from, to)
    })
}

pub fn compatibility_manifest(c_abi_version: CAbiVersion) -> CompatibilityManifest {
    let t = CompatibilityTable::current();
    CompatibilityManifest {
        schema_version: RELEASE_COMPATIBILITY_SCHEMA_VERSION.to_string(),
        engine_version: t.engine_version.to_string(),
        c_abi_version,
        pack_magic: t.pack_magic.to_string(),
        pack_schema_version: t.pack_schema_version,
        supported_pack_schemas: t.supported_pack_schemas.to_vec(),
        language_model_schema_version: t.language_model_schema_version,
        supported_language_model_schemas: t.supported_language_model_schemas.to_vec(),
        language_tag: t.language_tag.to_string(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceState {
    pub repository: String,
    pub commit: String,
    /// Git tree id of `data/` at that commit: the data revision.
    pub data_tree: String,
    pub worktree_dirty: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Toolchain {
    pub rust: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackRecord {
    pub pack_id: String,
    pub description: String,
    pub model_profile: String,
    pub is_default: bool,
    pub opt_in: bool,
    pub entry_count: usize,
    /// Artifact file name → SHA-256.
    pub files: BTreeMap<String, String>,
    pub pack_policy_sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_decisions_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_queue_manifest_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub controlled_review_report_manifest_sha256: Option<String>,
    pub source_provenance: Vec<SourceReviewProvenance>,
    pub data_licenses: Vec<DataLicenseEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language_model_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LanguageModelRecord {
    pub model_id: String,
    pub schema_version: String,
    pub manifest_sha256: String,
    pub vocabulary_fingerprint: String,
    pub vocabulary_size: usize,
    pub corpus_id: String,
    pub corpus_version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub corpus_source_artifact_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub corpus_documents_sha256: Option<String>,
    pub train_document_count: u64,
    pub train_document_set_sha256: String,
    pub bigram_min_count: u64,
    pub trigram_min_count: u64,
    /// Artifact file name → SHA-256.
    pub files: BTreeMap<String, String>,
    pub licensing: LanguageModelLicensing,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceRecord {
    pub source_id: String,
    pub source_name: String,
    pub version: String,
    pub license: String,
    pub license_url: String,
    pub url: String,
    pub redistribution: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CorpusRecord {
    pub corpus_id: String,
    pub corpus_name: String,
    pub version: String,
    pub license_spdx: String,
    pub license_url: String,
    pub url: String,
    pub acquisition: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_artifact_sha256: Option<String>,
    /// The registry's human determination for derivatives of this corpus (`pending-review`
    /// when none is recorded). Informational: the release gate is the language model's and
    /// the sources' determinations, which are what the bundle ships.
    pub redistribution_determination: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RedistributionRecord {
    pub subject: String,
    pub determination: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LicensingSummary {
    pub spdx_identifiers: Vec<String>,
    pub redistribution: Vec<RedistributionRecord>,
    pub notice: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlatformArtifact {
    pub platform: String,
    /// Bundle-relative path.
    pub path: String,
    pub sha256: String,
    pub size_bytes: u64,
    pub reproducibility: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseProvenance {
    pub schema_version: String,
    pub bundle_layout_version: String,
    pub release_version: String,
    /// `production` when every redistribution determination is `allowed`, else `evaluation`.
    pub release_kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evaluation_notice: Option<String>,
    pub language_tag: String,
    pub engine_version: String,
    pub c_abi_version: CAbiVersion,
    pub pack_schema_version: u32,
    pub language_model_schema_version: u32,
    pub source: SourceState,
    pub toolchain: Toolchain,
    pub packs: Vec<PackRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language_model: Option<LanguageModelRecord>,
    pub sources: Vec<SourceRecord>,
    pub corpora: Vec<CorpusRecord>,
    pub licensing: LicensingSummary,
    pub platform_artifacts: Vec<PlatformArtifact>,
    /// The verification the bundle was built from.
    pub production_state: ProductionStateReport,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReleaseBundleReport {
    pub bundle_dir: String,
    pub bundle_name: String,
    pub release_version: String,
    pub release_kind: String,
    pub file_count: usize,
    pub total_bytes: u64,
    /// SHA-256 of `SHA256SUMS`: the single identity of the whole bundle.
    pub sha256sums_sha256: String,
    pub provenance: ReleaseProvenance,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReleaseVerification {
    pub bundle_dir: String,
    pub file_count: usize,
    pub sha256sums_sha256: String,
    pub release_version: String,
    pub release_kind: String,
    pub provenance: ReleaseProvenance,
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn read(path: &Path) -> Result<Vec<u8>, String> {
    fs::read(path).map_err(|e| format!("Failed to read {:?}: {}", path, e))
}

fn git(root: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .map_err(|e| format!("git {:?} could not run: {}", args, e))?;
    if !output.status.success() {
        return Err(format!(
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn symlink_error(path: &Path) -> String {
    format!(
        "{:?} is a symbolic link; symbolic links are never followed (supply such artifacts as an archive, e.g. the XCFramework zip)",
        path
    )
}

/// The file type of `path` itself, never following a symbolic link.
fn file_type_no_follow(path: &Path) -> Result<fs::FileType, String> {
    fs::symlink_metadata(path)
        .map(|m| m.file_type())
        .map_err(|e| format!("{:?} does not exist or cannot be inspected: {}", path, e))
}

/// Every regular file under `dir`, as (path relative to `dir` with `/` separators, path).
/// Fails closed on any symbolic link below `dir`: nothing outside the tree is ever read.
fn walk_files(dir: &Path) -> Result<Vec<(String, PathBuf)>, String> {
    fn walk(dir: &Path, rel: &str, out: &mut Vec<(String, PathBuf)>) -> Result<(), String> {
        let rd = fs::read_dir(dir).map_err(|e| format!("Failed to list {:?}: {}", dir, e))?;
        let mut entries: Vec<_> = rd
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("Failed to list {:?}: {}", dir, e))?;
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let name = e.file_name().to_string_lossy().to_string();
            let child_rel = if rel.is_empty() {
                name.clone()
            } else {
                format!("{}/{}", rel, name)
            };
            let path = e.path();
            let file_type = fs::symlink_metadata(&path)
                .map_err(|e| format!("Failed to inspect {:?}: {}", path, e))?
                .file_type();
            if file_type.is_symlink() {
                return Err(symlink_error(&path));
            } else if file_type.is_dir() {
                walk(&path, &child_rel, out)?;
            } else if file_type.is_file() {
                out.push((child_rel, path));
            } else {
                return Err(format!(
                    "{:?} is neither a regular file nor a directory",
                    path
                ));
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    walk(dir, "", &mut out)?;
    Ok(out)
}

fn rust_toolchain_channel(root: &Path) -> Result<String, String> {
    let text = fs::read_to_string(root.join("rust-toolchain.toml"))
        .map_err(|e| format!("Failed to read rust-toolchain.toml: {}", e))?;
    text.lines()
        .filter_map(|l| l.trim().strip_prefix("channel"))
        .filter_map(|rest| rest.trim().strip_prefix('='))
        .map(|v| v.trim().trim_matches('"').to_string())
        .next()
        .ok_or_else(|| "rust-toolchain.toml has no channel".to_string())
}

fn validate_release_version(version: &str) -> Result<(), String> {
    let ok = !version.is_empty()
        && version
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+'));
    if ok {
        Ok(())
    } else {
        Err(format!(
            "release version {:?} may only contain ASCII letters, digits, '.', '-' and '+'",
            version
        ))
    }
}

fn add_file(
    files: &mut BTreeMap<String, Vec<u8>>,
    rel: String,
    bytes: Vec<u8>,
) -> Result<(), String> {
    if rel.is_empty() || rel.starts_with('/') || rel.split('/').any(|c| c == ".." || c.is_empty()) {
        return Err(format!("invalid bundle path {:?}", rel));
    }
    if files.insert(rel.clone(), bytes).is_some() {
        return Err(format!("bundle path {:?} would be written twice", rel));
    }
    Ok(())
}

fn attach_platform_artifacts(
    platform: &str,
    paths: &[PathBuf],
    files: &mut BTreeMap<String, Vec<u8>>,
    records: &mut Vec<PlatformArtifact>,
) -> Result<(), String> {
    for path in paths {
        let name = path
            .file_name()
            .ok_or_else(|| format!("{:?} has no file name", path))?
            .to_string_lossy()
            .to_string();
        let mut entries: Vec<(String, Vec<u8>)> = Vec::new();
        let file_type =
            file_type_no_follow(path).map_err(|e| format!("{} artifact: {}", platform, e))?;
        if file_type.is_symlink() {
            return Err(format!("{} artifact: {}", platform, symlink_error(path)));
        } else if file_type.is_dir() {
            for (rel, file) in walk_files(path)? {
                entries.push((format!("{}/{}/{}", platform, name, rel), read(&file)?));
            }
        } else if file_type.is_file() {
            entries.push((format!("{}/{}", platform, name), read(path)?));
        } else {
            return Err(format!(
                "{} artifact {:?} is neither a regular file nor a directory",
                platform, path
            ));
        }
        for (rel, bytes) in entries {
            records.push(PlatformArtifact {
                platform: platform.to_string(),
                path: rel.clone(),
                sha256: sha256_hex(&bytes),
                size_bytes: bytes.len() as u64,
                reproducibility: format!(
                    "not asserted by this bundle; built by scripts/{} and hashed as received",
                    platform
                ),
            });
            add_file(files, rel, bytes)?;
        }
    }
    Ok(())
}

fn json_str(v: &serde_json::Value, key: &str) -> Option<String> {
    v.get(key).and_then(|x| x.as_str()).map(|s| s.to_string())
}

/// Verifies the production state, then builds the bundle under `out`.
pub fn build_release_bundle<P: AsRef<Path>, Q: AsRef<Path>>(
    root: P,
    options: &ReleaseOptions,
    out: Q,
) -> Result<ReleaseBundleReport, String> {
    let state = verify_production_state(root.as_ref())?;
    build_release_bundle_from_state(root, &state, options, out)
}

/// Builds the bundle from an already computed production-state report (must be `ok`).
pub fn build_release_bundle_from_state<P: AsRef<Path>, Q: AsRef<Path>>(
    root: P,
    state: &ProductionStateReport,
    options: &ReleaseOptions,
    out: Q,
) -> Result<ReleaseBundleReport, String> {
    let root = root.as_ref();
    let out = out.as_ref();

    // Preconditions: verified state, every pack built and current, a clean tracked tree.
    if !state.ok {
        return Err(format!(
            "production state is not OK; nothing was written:\n{}",
            render_state_text(state)
        ));
    }
    for p in &state.packs {
        if p.built_matches != Some(true) {
            return Err(format!(
                "pack '{}' is not built and current under data/build/packs; run build-pack (nothing was written)",
                p.pack_id
            ));
        }
    }
    let release_version = options
        .release_version
        .clone()
        .unwrap_or_else(|| ENGINE_VERSION.to_string());
    validate_release_version(&release_version)?;
    let commit = git(root, &["rev-parse", "HEAD"])?;
    let data_tree = git(root, &["rev-parse", "HEAD:data"])?;
    // Untracked (non-ignored) files count as dirty: the bundle consumes files such as NOTICE
    // and data/licenses/, so an untracked one would change the bytes without a commit.
    let worktree_dirty =
        !git(root, &["status", "--porcelain", "--untracked-files=all"])?.is_empty();
    if worktree_dirty && !options.allow_dirty {
        return Err(
            "the working tree has uncommitted or untracked (non-ignored) files; commit or remove them, or pass --allow-dirty (the provenance then records worktree_dirty = true and the bundle is an evaluation release); nothing was written"
                .to_string(),
        );
    }

    let policy = PackPolicyConfig::load_from_file(root.join("data/pack-policy.toml"))?;
    let sources = SourceRegistry::load_from_file(root.join("data/source-registry/sources.toml"))?;
    let corpora = CorpusRegistry::load_from_file(root.join("data/source-registry/corpora.toml"))?;
    let header = read(&root.join(C_HEADER_PATH))?;
    let c_abi_version = parse_c_abi_version(&String::from_utf8_lossy(&header))?;
    let compatibility = compatibility_manifest(c_abi_version.clone());

    let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    add_file(
        &mut files,
        "VERSION".to_string(),
        format!("{}\n", release_version).into_bytes(),
    )?;
    add_file(&mut files, "include/kurmanci.h".to_string(), header)?;

    // Packs: the five verified artifacts of each, straight from the build directory.
    let mut packs = Vec::new();
    let mut attribution = String::new();
    let mut spdx: BTreeSet<String> = BTreeSet::new();
    for p in &state.packs {
        let pack_id = &p.pack_id;
        let def = policy
            .packs
            .get(pack_id)
            .ok_or_else(|| format!("pack '{}' missing from pack policy", pack_id))?;
        let pack_dir = root.join(format!("data/build/packs/{}", pack_id));
        let mut hashes = BTreeMap::new();
        let mut manifest: Option<PackManifest> = None;
        for name in PACK_ARTIFACT_FILES {
            let bytes = read(&pack_dir.join(name))?;
            if name == "manifest.json" {
                manifest = Some(
                    serde_json::from_slice(&bytes)
                        .map_err(|e| format!("pack '{}' manifest.json: {}", pack_id, e))?,
                );
            }
            if name == "attribution.txt" {
                attribution.push_str(&format!(
                    "# Pack {}\n\n{}\n\n",
                    pack_id,
                    String::from_utf8_lossy(&bytes).trim_end()
                ));
            }
            hashes.insert(name.to_string(), sha256_hex(&bytes));
            add_file(&mut files, format!("packs/{}/{}", pack_id, name), bytes)?;
        }
        let manifest = manifest.ok_or_else(|| format!("pack '{}' has no manifest", pack_id))?;
        for l in &manifest.data_licenses {
            spdx.insert(l.spdx.clone());
        }
        packs.push(PackRecord {
            pack_id: pack_id.clone(),
            description: def.description.clone(),
            model_profile: manifest.model_profile.clone(),
            is_default: manifest.is_default,
            opt_in: manifest.is_experimental,
            entry_count: manifest.final_unique_entry_count,
            files: hashes,
            pack_policy_sha256: manifest.pack_policy_sha256.clone(),
            review_decisions_sha256: manifest.review_decisions_sha256.clone(),
            review_queue_manifest_sha256: manifest.review_queue_manifest_sha256.clone(),
            controlled_review_report_manifest_sha256: manifest
                .controlled_review_report_manifest_sha256
                .clone(),
            source_provenance: manifest.source_provenance.clone(),
            data_licenses: manifest.data_licenses.clone(),
            language_model_id: manifest.language_model_id.clone(),
        });
    }
    add_file(
        &mut files,
        "ATTRIBUTION".to_string(),
        attribution.into_bytes(),
    )?;

    // Language model: the committed artifacts (non-prose statistics only).
    let mut redistribution = Vec::new();
    let mut language_model = None;
    if let Some(lm) = &state.language_model {
        let model = load_language_model(root, &lm.model_id)?;
        let model_dir = root.join(LANGUAGE_MODEL_DIR).join(&lm.model_id);
        let mut hashes = BTreeMap::new();
        for name in LANGUAGE_MODEL_FILES {
            let bytes = read(&model_dir.join(name))?;
            hashes.insert(name.to_string(), sha256_hex(&bytes));
            add_file(
                &mut files,
                format!("language-model/{}/{}", lm.model_id, name),
                bytes,
            )?;
        }
        let m = serde_json::to_value(&model.manifest).map_err(|e| e.to_string())?;
        spdx.insert(model.manifest.licensing.license_spdx.clone());
        redistribution.push(RedistributionRecord {
            subject: format!("language-model:{}", lm.model_id),
            determination: model
                .manifest
                .licensing
                .redistribution_determination
                .clone(),
        });
        language_model = Some(LanguageModelRecord {
            model_id: lm.model_id.clone(),
            schema_version: model.manifest.schema_version.clone(),
            manifest_sha256: model.manifest_sha256.clone(),
            vocabulary_fingerprint: model.manifest.vocabulary_fingerprint.clone(),
            vocabulary_size: model.manifest.vocabulary_size,
            corpus_id: json_str(&m, "corpus_id").unwrap_or_default(),
            corpus_version: json_str(&m, "corpus_version").unwrap_or_default(),
            corpus_source_artifact_sha256: json_str(&m, "corpus_source_artifact_sha256"),
            corpus_documents_sha256: json_str(&m, "corpus_documents_sha256"),
            train_document_count: m
                .get("train_document_count")
                .and_then(|v| v.as_u64())
                .unwrap_or(0),
            train_document_set_sha256: json_str(&m, "train_document_set_sha256")
                .unwrap_or_default(),
            bigram_min_count: m
                .get("bigram_min_count")
                .and_then(|v| v.as_u64())
                .unwrap_or(0),
            trigram_min_count: m
                .get("trigram_min_count")
                .and_then(|v| v.as_u64())
                .unwrap_or(0),
            files: hashes,
            licensing: model.manifest.licensing.clone(),
        });
    }

    // Registries and licences.
    let mut source_records = Vec::new();
    for s in &sources.sources {
        spdx.insert(s.license.clone());
        redistribution.push(RedistributionRecord {
            subject: format!("source:{}", s.source_id),
            determination: s.redistribution.clone(),
        });
        source_records.push(SourceRecord {
            source_id: s.source_id.clone(),
            source_name: s.source_name.clone(),
            version: s.version.clone(),
            license: s.license.clone(),
            license_url: s.license_url.clone(),
            url: s.url.clone(),
            redistribution: s.redistribution.clone(),
        });
    }
    let corpus_records: Vec<CorpusRecord> = corpora
        .corpora
        .iter()
        .map(|c| CorpusRecord {
            corpus_id: c.corpus_id.clone(),
            corpus_name: c.corpus_name.clone(),
            version: c.version.clone(),
            license_spdx: c.license_spdx.clone(),
            license_url: c.license_url.clone(),
            url: c.url.clone(),
            acquisition: c.acquisition.clone(),
            source_artifact_sha256: c.source_artifact.as_ref().map(|a| a.sha256.clone()),
            redistribution_determination: c.redistribution_determination().to_string(),
        })
        .collect();
    add_file(
        &mut files,
        "LICENSES/LICENSE".to_string(),
        read(&root.join("LICENSE"))?,
    )?;
    if root.join("NOTICE").is_file() {
        add_file(
            &mut files,
            "LICENSES/NOTICE".to_string(),
            read(&root.join("NOTICE"))?,
        )?;
    }
    let licenses_dir = root.join("data/licenses");
    if licenses_dir.is_dir() {
        for (rel, path) in walk_files(&licenses_dir)? {
            add_file(&mut files, format!("LICENSES/{}", rel), read(&path)?)?;
        }
    }

    // Platform artifacts, hashed as received.
    let mut platform_artifacts = Vec::new();
    attach_platform_artifacts(
        "apple",
        &options.apple_artifacts,
        &mut files,
        &mut platform_artifacts,
    )?;
    attach_platform_artifacts(
        "android",
        &options.android_artifacts,
        &mut files,
        &mut platform_artifacts,
    )?;

    let ReleaseClassification {
        release_kind,
        evaluation_notice,
    } = classify_release(&redistribution, worktree_dirty);
    let licensing = LicensingSummary {
        spdx_identifiers: spdx.into_iter().collect(),
        redistribution,
        notice: "Licence identifiers, URLs and redistribution determinations are recorded as declared in data/source-registry and the language model manifest. Per-source licence files are under LICENSES/; ATTRIBUTION carries every pack's attribution text.".to_string(),
    };
    let provenance = ReleaseProvenance {
        schema_version: RELEASE_PROVENANCE_SCHEMA_VERSION.to_string(),
        bundle_layout_version: RELEASE_BUNDLE_LAYOUT_VERSION.to_string(),
        release_version: release_version.clone(),
        release_kind: release_kind.clone(),
        evaluation_notice,
        language_tag: compatibility.language_tag.clone(),
        engine_version: compatibility.engine_version.clone(),
        c_abi_version,
        pack_schema_version: compatibility.pack_schema_version,
        language_model_schema_version: compatibility.language_model_schema_version,
        source: SourceState {
            repository: SOURCE_REPOSITORY.to_string(),
            commit,
            data_tree,
            worktree_dirty,
        },
        toolchain: Toolchain {
            rust: rust_toolchain_channel(root)?,
        },
        packs,
        language_model,
        sources: source_records,
        corpora: corpus_records,
        licensing,
        platform_artifacts,
        production_state: state.clone(),
    };
    add_file(
        &mut files,
        "compatibility.json".to_string(),
        (serde_json::to_string_pretty(&compatibility).map_err(|e| e.to_string())? + "\n")
            .into_bytes(),
    )?;
    add_file(
        &mut files,
        "provenance.json".to_string(),
        (serde_json::to_string_pretty(&provenance).map_err(|e| e.to_string())? + "\n").into_bytes(),
    )?;
    let mut sums = String::new();
    for (rel, bytes) in &files {
        use std::fmt::Write;
        writeln!(sums, "{}  {}", sha256_hex(bytes), rel).map_err(|e| e.to_string())?;
    }
    let sha256sums_sha256 = sha256_hex(sums.as_bytes());
    add_file(&mut files, "SHA256SUMS".to_string(), sums.into_bytes())?;

    // Failure-safe staged replacement: stage the complete bundle, park any previous bundle
    // as a backup, rename the stage into place, restore the backup if that fails.
    let bundle_name = format!("kurmanci-{}-{}", SUPPORTED_LANGUAGE_TAG, release_version);
    let bundle_dir = out.join(&bundle_name);
    let stage_dir = out.join(format!(".{}.tmp-stage", bundle_name));
    let backup_dir = out.join(format!(".{}.tmp-backup", bundle_name));
    fs::create_dir_all(out).map_err(|e| format!("Failed to create {:?}: {}", out, e))?;
    if stage_dir.exists() {
        fs::remove_dir_all(&stage_dir).map_err(|e| format!("Failed to clean stage: {}", e))?;
    }
    let mut total_bytes = 0u64;
    for (rel, bytes) in &files {
        let path = stage_dir.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create {:?}: {}", parent, e))?;
        }
        fs::write(&path, bytes).map_err(|e| format!("Failed to write {:?}: {}", path, e))?;
        total_bytes += bytes.len() as u64;
    }
    install_directory(&stage_dir, &bundle_dir, &backup_dir)?;

    Ok(ReleaseBundleReport {
        bundle_dir: bundle_dir.display().to_string(),
        bundle_name,
        release_version,
        release_kind,
        file_count: files.len(),
        total_bytes,
        sha256sums_sha256,
        provenance,
    })
}

/// Re-checks a bundle directory without writing anything: every file listed in SHA256SUMS
/// exists with that hash, no unlisted file exists, and the provenance, compatibility table,
/// VERSION, pack manifests and language-model manifest agree with the file hashes.
pub fn verify_release_bundle<P: AsRef<Path>>(dir: P) -> Result<ReleaseVerification, String> {
    let dir = dir.as_ref();
    let root_type = file_type_no_follow(dir)?;
    if root_type.is_symlink() {
        return Err(symlink_error(dir));
    }
    if !root_type.is_dir() {
        return Err(format!("{:?} is not a directory", dir));
    }
    let sums_bytes = read(&dir.join("SHA256SUMS"))?;
    let sums_text =
        String::from_utf8(sums_bytes.clone()).map_err(|_| "SHA256SUMS is not UTF-8".to_string())?;
    let mut listed: BTreeMap<String, String> = BTreeMap::new();
    for (idx, line) in sums_text.lines().enumerate() {
        let (sha, rel) = line
            .split_once("  ")
            .ok_or_else(|| format!("SHA256SUMS line {} is malformed", idx + 1))?;
        if sha.len() != 64 || !sha.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(format!("SHA256SUMS line {} has an invalid hash", idx + 1));
        }
        if listed.insert(rel.to_string(), sha.to_string()).is_some() {
            return Err(format!("SHA256SUMS lists {:?} twice", rel));
        }
    }
    let present: BTreeMap<String, PathBuf> = walk_files(dir)?.into_iter().collect();
    let mut extra: Vec<&String> = present
        .keys()
        .filter(|k| k.as_str() != "SHA256SUMS" && !listed.contains_key(*k))
        .collect();
    extra.sort();
    if !extra.is_empty() {
        return Err(format!("files not listed in SHA256SUMS: {:?}", extra));
    }
    for (rel, sha) in &listed {
        let path = present
            .get(rel)
            .ok_or_else(|| format!("{} is listed in SHA256SUMS but missing", rel))?;
        let actual = sha256_hex(&read(path)?);
        if &actual != sha {
            return Err(format!(
                "{} does not match SHA256SUMS ({} on disk vs {} listed)",
                rel,
                &actual[..12],
                &sha[..12]
            ));
        }
    }

    let compatibility: CompatibilityManifest =
        serde_json::from_slice(&read(&dir.join("compatibility.json"))?)
            .map_err(|e| format!("compatibility.json: {}", e))?;
    let provenance: ReleaseProvenance =
        serde_json::from_slice(&read(&dir.join("provenance.json"))?)
            .map_err(|e| format!("provenance.json: {}", e))?;
    let version = String::from_utf8_lossy(&read(&dir.join("VERSION"))?)
        .trim()
        .to_string();
    if version != provenance.release_version {
        return Err(format!(
            "VERSION ({}) differs from provenance release_version ({})",
            version, provenance.release_version
        ));
    }
    let disagreements: Vec<&str> = [
        (
            compatibility.engine_version != provenance.engine_version,
            "engine_version",
        ),
        (
            compatibility.language_tag != provenance.language_tag,
            "language_tag",
        ),
        (
            compatibility.c_abi_version != provenance.c_abi_version,
            "c_abi_version",
        ),
        (
            compatibility.pack_schema_version != provenance.pack_schema_version,
            "pack_schema_version",
        ),
        (
            compatibility.language_model_schema_version != provenance.language_model_schema_version,
            "language_model_schema_version",
        ),
    ]
    .iter()
    .filter(|(differs, _)| *differs)
    .map(|(_, name)| *name)
    .collect();
    if !disagreements.is_empty() {
        return Err(format!(
            "compatibility.json and provenance.json disagree on {}",
            disagreements.join(", ")
        ));
    }
    if !provenance.production_state.ok {
        return Err("provenance records a production state that was not OK".to_string());
    }
    if provenance.source.worktree_dirty && provenance.release_kind == "production" {
        return Err(
            "provenance records worktree_dirty = true together with release_kind = \"production\"; a bundle built from uncommitted changes can only be an evaluation release"
                .to_string(),
        );
    }
    let expected = classify_release(
        &provenance.licensing.redistribution,
        provenance.source.worktree_dirty,
    );
    if provenance.release_kind != expected.release_kind {
        return Err(format!(
            "release_kind {:?} does not follow from the recorded redistribution determinations and dirty state (expected {:?})",
            provenance.release_kind, expected.release_kind
        ));
    }
    if provenance.release_kind == "evaluation"
        && provenance
            .evaluation_notice
            .as_deref()
            .map(|n| n.trim().is_empty())
            .unwrap_or(true)
    {
        return Err("an evaluation release must carry an evaluation notice".to_string());
    }
    let expect = |rel: String, sha: &str| -> Result<(), String> {
        match listed.get(&rel) {
            Some(s) if s == sha => Ok(()),
            Some(_) => Err(format!(
                "{} hash in provenance differs from SHA256SUMS",
                rel
            )),
            None => Err(format!("{} named in provenance is not in the bundle", rel)),
        }
    };
    for p in &provenance.packs {
        for (name, sha) in &p.files {
            expect(format!("packs/{}/{}", p.pack_id, name), sha)?;
        }
        let manifest: PackManifest = serde_json::from_slice(&read(
            &dir.join(format!("packs/{}/manifest.json", p.pack_id)),
        )?)
        .map_err(|e| format!("packs/{}/manifest.json: {}", p.pack_id, e))?;
        if Some(&manifest.binary_sha256) != p.files.get("lexicon.bin") {
            return Err(format!(
                "packs/{}/manifest.json binary_sha256 differs from lexicon.bin",
                p.pack_id
            ));
        }
        if manifest.final_unique_entry_count != p.entry_count {
            return Err(format!(
                "packs/{} entry count differs from provenance",
                p.pack_id
            ));
        }
    }
    if let Some(lm) = &provenance.language_model {
        for (name, sha) in &lm.files {
            expect(format!("language-model/{}/{}", lm.model_id, name), sha)?;
        }
        if lm.files.get("manifest.json") != Some(&lm.manifest_sha256) {
            return Err("language model manifest hash differs from provenance".to_string());
        }
    }
    for a in &provenance.platform_artifacts {
        expect(a.path.clone(), &a.sha256)?;
    }
    Ok(ReleaseVerification {
        bundle_dir: dir.display().to_string(),
        file_count: present.len(),
        sha256sums_sha256: sha256_hex(&sums_bytes),
        release_version: provenance.release_version.clone(),
        release_kind: provenance.release_kind.clone(),
        provenance,
    })
}

pub fn render_release_text(r: &ReleaseBundleReport) -> String {
    let mut s = format!(
        "release bundle: {}\n  version: {} ({} release)\n  files: {} ({} bytes)\n  SHA256SUMS sha256: {}\n  source: {} @ {}{}\n  data tree: {}\n",
        r.bundle_dir,
        r.release_version,
        r.release_kind,
        r.file_count,
        r.total_bytes,
        r.sha256sums_sha256,
        r.provenance.source.repository,
        r.provenance.source.commit,
        if r.provenance.source.worktree_dirty {
            " (worktree dirty)"
        } else {
            ""
        },
        r.provenance.source.data_tree
    );
    for p in &r.provenance.packs {
        s.push_str(&format!(
            "  pack {:<18} {} entries  lexicon.bin {}\n",
            p.pack_id,
            p.entry_count,
            &p.files["lexicon.bin"][..12]
        ));
    }
    if let Some(lm) = &r.provenance.language_model {
        s.push_str(&format!(
            "  language model {}  manifest {}  redistribution {}\n",
            lm.model_id,
            &lm.manifest_sha256[..12],
            lm.licensing.redistribution_determination
        ));
    }
    for a in &r.provenance.platform_artifacts {
        s.push_str(&format!(
            "  {} artifact {} {}\n",
            a.platform,
            a.path,
            &a.sha256[..12]
        ));
    }
    if let Some(n) = &r.provenance.evaluation_notice {
        s.push_str(&format!("  notice: {}\n", n));
    }
    s
}

pub fn render_verification_text(v: &ReleaseVerification) -> String {
    format!(
        "release bundle OK: {}\n  version: {} ({} release)\n  files: {} verified against SHA256SUMS\n  SHA256SUMS sha256: {}\n  packs: {}\n",
        v.bundle_dir,
        v.release_version,
        v.release_kind,
        v.file_count,
        v.sha256sums_sha256,
        v.provenance
            .packs
            .iter()
            .map(|p| format!("{} {}", p.pack_id, &p.files["lexicon.bin"][..12]))
            .collect::<Vec<_>>()
            .join(", ")
    )
}
