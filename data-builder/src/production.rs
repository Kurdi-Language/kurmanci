//! Production state: one read-only verification and one explicit rebuild.
//!
//! `verify_production_state` checks everything a contributor or vendor must be able to trust
//! before using the repository's packs, and changes nothing. Every check runs; the report
//! lists each one as pass, fail or skipped (with the reason), and `ok` is true only when no
//! check failed. Pack reproducibility is verified by assembling the complete artifact set of
//! every pack in memory, twice, and comparing every file with the built artifact when one
//! exists — through the same assembly path `build-pack` writes, so nothing is reimplemented.
//!
//! The checks are split into two groups. *Authoritative inputs* are what humans and the
//! registries decide (source and corpus registries, pack policy, review artifacts and
//! decisions, trust subsets, the policy's model set); a rebuild cannot repair them, so
//! `rebuild_production` verifies exactly this group before it mutates anything and refuses
//! otherwise. *Derived outputs* (the committed language model, the built packs, the local
//! corpus pipeline) are what a rebuild regenerates.
//!
//! `rebuild_production` is the deliberate, mutating counterpart: it regenerates the
//! corpus-derived statistics, the committed language model and the packs from the human
//! decisions already committed, then verifies. It never creates, changes or promotes a
//! vocabulary decision, never touches review artifacts, and never downloads anything unless
//! explicitly asked (`acquire = true`). Repeated runs are byte-identical.

use crate::corpus::acquire::acquire_corpus;
use crate::corpus::importer::{import_all_corpora, verify_canonical_manifest};
use crate::corpus::partition::{partition_corpora, PartitionBuildManifest};
use crate::corpus::registry::CorpusRegistry;
use crate::pack::builder::{
    assemble_pack_artifacts, build_pack, resolve_authoritative_pack_lexicon, PACK_ARTIFACT_FILES,
};
use crate::pack::language_model::{
    build_language_model, language_model_manifest_sha256, load_language_model, LANGUAGE_MODEL_DIR,
    PRODUCTION_LANGUAGE_MODEL_BUILD,
};
use crate::pack::manifest::validate_all_pack_manifests;
use crate::pack::policy::PackPolicyConfig;
use crate::review::kuwiki_decisions::load_and_validate_all_kuwiki_decisions;
use crate::review::merger::load_validated_review_snapshot;
use crate::review::schema::ReviewDecisionRecord;
use crate::sources::SourceRegistry;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::Path;

pub const PRODUCTION_STATE_SCHEMA_VERSION: &str = "production-state-v1";
pub const PACK_IDS: [&str; 3] = ["seed", "reviewed", "experimental-full"];
const HUNSPELL_SOURCE_ID: &str = "kurdish-hunspell-kmr";
const KUWIKI_CORPUS_ID: &str = "kuwiki";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckStatus {
    Pass,
    Fail,
    Skipped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Check {
    pub name: String,
    pub status: CheckStatus,
    pub detail: String,
}

/// One file of a built pack, compared between the in-memory assembly and the build directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PackArtifactState {
    pub name: String,
    pub in_memory_sha256: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub built_sha256: Option<String>,
    /// Whether the file under `data/build/packs/` equals the assembly; `None` when not built.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub built_matches: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PackReproducibility {
    pub pack_id: String,
    /// SHA-256 of the in-memory `lexicon.bin`.
    pub in_memory_sha256: String,
    pub entry_count: usize,
    /// SHA-256 of the built `lexicon.bin`, when built.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub built_sha256: Option<String>,
    /// Whether every built artifact equals the in-memory assembly; `None` when not built.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub built_matches: Option<bool>,
    pub artifacts: Vec<PackArtifactState>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LanguageModelState {
    pub model_id: String,
    pub manifest_sha256: String,
    pub vocabulary_fingerprint: String,
    pub vocabulary_size: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProductionStateReport {
    pub schema_version: String,
    pub ok: bool,
    pub checks: Vec<Check>,
    pub packs: Vec<PackReproducibility>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language_model: Option<LanguageModelState>,
}

/// Abbreviates an id or hash for diagnostics without ever panicking: shorter or non-ASCII
/// values (a malformed target id is still a valid decision record) come back whole.
pub fn abbrev(s: &str) -> &str {
    s.get(..12).unwrap_or(s)
}

fn sha256_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn sha256_file(path: &Path) -> Result<String, String> {
    fs::read(path)
        .map(|b| sha256_bytes(&b))
        .map_err(|e| format!("Failed to read {:?}: {}", path, e))
}

fn read_jsonl<T: serde::de::DeserializeOwned>(path: &Path) -> Result<Vec<T>, String> {
    let file = File::open(path).map_err(|e| format!("Failed to open {:?}: {}", path, e))?;
    let mut out = Vec::new();
    for (idx, line) in BufReader::new(file).lines().enumerate() {
        let line = line.map_err(|e| format!("Read error in {:?} line {}: {}", path, idx + 1, e))?;
        if line.trim().is_empty() {
            continue;
        }
        out.push(
            serde_json::from_str(&line)
                .map_err(|e| format!("JSON error in {:?} line {}: {}", path, idx + 1, e))?,
        );
    }
    Ok(out)
}

struct Checks(Vec<Check>);

impl Checks {
    fn record(&mut self, name: &str, result: Result<String, String>) {
        let check = match result {
            Ok(detail) => Check {
                name: name.to_string(),
                status: CheckStatus::Pass,
                detail,
            },
            Err(detail) => Check {
                name: name.to_string(),
                status: CheckStatus::Fail,
                detail,
            },
        };
        self.0.push(check);
    }

    fn skip(&mut self, name: &str, reason: &str) {
        self.0.push(Check {
            name: name.to_string(),
            status: CheckStatus::Skipped,
            detail: reason.to_string(),
        });
    }
}

/// Reports duplicate decision identities (`source_id`, `target_id`) as a failure, listing
/// each duplicate abbreviated; malformed or short target ids are reported, never sliced.
pub fn duplicate_identity_report(
    identities: &BTreeMap<(String, String), usize>,
) -> Result<String, String> {
    let duplicates: Vec<String> = identities
        .iter()
        .filter(|(_, n)| **n > 1)
        .map(|((s, t), n)| format!("{}:{} x{}", s, abbrev(t), n))
        .collect();
    if duplicates.is_empty() {
        Ok(format!(
            "{} decision identities, all unique",
            identities.len()
        ))
    } else {
        Err(format!(
            "duplicate decision identities: {}",
            duplicates.join(", ")
        ))
    }
}

/// The model ids `rebuild_production` can generate from the registered corpora: exactly one
/// per registered Kuwiki corpus (`kuwiki-<version>`), nothing else.
fn producible_model_ids(corpora: &CorpusRegistry) -> BTreeSet<String> {
    corpora
        .find_corpus(KUWIKI_CORPUS_ID)
        .map(|k| BTreeSet::from([format!("{}-{}", KUWIKI_CORPUS_ID, k.version)]))
        .unwrap_or_default()
}

fn policy_model_ids(policy: &PackPolicyConfig) -> BTreeSet<String> {
    policy
        .packs
        .values()
        .filter_map(|d| d.language_model.clone())
        .collect()
}

/// Result of verifying the authoritative inputs (see the module documentation).
pub struct AuthoritativeInputs {
    pub checks: Vec<Check>,
    pub corpora: Option<CorpusRegistry>,
    pub policy: Option<PackPolicyConfig>,
}

impl AuthoritativeInputs {
    pub fn ok(&self) -> bool {
        self.checks.iter().all(|c| c.status != CheckStatus::Fail)
    }
}

/// Verifies, without writing anything, every input that a rebuild cannot legitimately repair:
/// the registries and policy, the Hunspell and Kuwiki review artifacts and decisions,
/// decision-identity uniqueness, the trust subsets and the policy's language-model set.
/// Committed model artifacts and built packs are deliberately not part of this group.
pub fn check_authoritative_inputs<P: AsRef<Path>>(root: P) -> AuthoritativeInputs {
    let root = root.as_ref();
    let mut checks = Checks(Vec::new());

    // 1. Registries and policy.
    let sources_path = root.join("data/source-registry/sources.toml");
    let corpora_path = root.join("data/source-registry/corpora.toml");
    checks.record(
        "source-registry",
        SourceRegistry::load_from_file(&sources_path)
            .map(|r| format!("{} sources registered", r.sources.len())),
    );
    let corpora = CorpusRegistry::load_from_file(&corpora_path);
    checks.record(
        "corpus-registry",
        corpora
            .as_ref()
            .map(|r| format!("{} corpora registered", r.corpora.len()))
            .map_err(|e| e.clone()),
    );
    let policy = PackPolicyConfig::load_from_file(root.join("data/pack-policy.toml"));
    checks.record(
        "pack-policy",
        policy
            .as_ref()
            .map(|p| {
                let profiles: Vec<String> = p
                    .packs
                    .iter()
                    .map(|(id, d)| format!("{}={}", id, d.model_profile))
                    .collect();
                format!("default {}; {}", p.default_pack, profiles.join(", "))
            })
            .map_err(|e| e.clone()),
    );

    // 2. Review artifacts: Hunspell queues, reports and decisions.
    let mut identities: BTreeMap<(String, String), usize> = BTreeMap::new();
    match load_validated_review_snapshot(HUNSPELL_SOURCE_ID, root) {
        Ok(summary) => {
            let decisions_path = root.join(format!(
                "data/review-decisions/{}/decisions.jsonl",
                HUNSPELL_SOURCE_ID
            ));
            match read_jsonl::<ReviewDecisionRecord>(&decisions_path) {
                Ok(decisions) => {
                    for d in &decisions {
                        *identities
                            .entry((d.source_id.clone(), d.target_id.clone()))
                            .or_insert(0) += 1;
                    }
                    let mut detail = format!(
                        "{} decisions; queue and report manifests verified",
                        decisions.len()
                    );
                    let result = if summary.orphan_decisions_count > 0 {
                        Err(format!(
                            "{} orphan decisions (target not in any queue)",
                            summary.orphan_decisions_count
                        ))
                    } else {
                        if summary.unresolved_count > 0 {
                            detail.push_str(&format!(
                                "; {} unresolved conflict groups pending",
                                summary.unresolved_count
                            ));
                        }
                        Ok(detail)
                    };
                    checks.record("hunspell-review", result);
                }
                Err(e) => checks.record("hunspell-review", Err(e)),
            }
        }
        Err(e) => checks.record("hunspell-review", Err(e)),
    }

    // 3. Kuwiki review batches: artifact hashes, counts, no-repeat, decision provenance.
    let kuwiki_registered = corpora
        .as_ref()
        .map(|r| r.find_corpus(KUWIKI_CORPUS_ID).is_some())
        .unwrap_or(false);
    let mut batch_normalized: BTreeMap<String, String> = BTreeMap::new();
    if kuwiki_registered {
        match load_and_validate_all_kuwiki_decisions(root) {
            Ok(snapshots) => {
                let mut result = Ok(String::new());
                let mut total = 0usize;
                for s in &snapshots {
                    total += s.decisions.len();
                    for d in &s.decisions {
                        *identities
                            .entry((d.source_id.clone(), d.target_id.clone()))
                            .or_insert(0) += 1;
                    }
                    for c in &s.candidates {
                        if !c.context_references.is_empty() {
                            result = Err(format!(
                                "{} candidate rank {} carries context references (corpus context is forbidden in tracked review artifacts)",
                                s.batch_id, c.batch_rank
                            ));
                        }
                        if let Some(prev) =
                            batch_normalized.insert(c.normalized_token.clone(), s.batch_id.clone())
                        {
                            if prev != s.batch_id {
                                result = Err(format!(
                                    "normalized token {:?} appears in both {} and {}",
                                    c.normalized_token, prev, s.batch_id
                                ));
                            }
                        }
                    }
                }
                if result.is_ok() {
                    result = Ok(format!(
                        "{} batches, {} decisions, {} candidates, artifacts verified, no cross-batch repeats, no context references",
                        snapshots.len(),
                        total,
                        batch_normalized.len()
                    ));
                }
                checks.record("kuwiki-review", result);
            }
            Err(e) => checks.record("kuwiki-review", Err(e)),
        }
    } else {
        checks.skip("kuwiki-review", "kuwiki is not registered in corpora.toml");
    }

    // 4. Review identities are unique across every source and batch.
    checks.record(
        "review-identities-unique",
        duplicate_identity_report(&identities),
    );

    // 5. Trust subsets: seed ⊆ reviewed ⊆ experimental-full (by normalized form).
    let mut pack_sets: BTreeMap<&str, BTreeSet<String>> = BTreeMap::new();
    let mut subset_result = Ok(String::new());
    for pack_id in PACK_IDS {
        match resolve_authoritative_pack_lexicon(pack_id, root) {
            Ok(entries) => {
                pack_sets.insert(pack_id, entries.into_iter().map(|e| e.normalized).collect());
            }
            Err(e) => {
                subset_result = Err(format!("cannot resolve pack '{}': {}", pack_id, e));
            }
        }
    }
    if subset_result.is_ok() {
        let seed = &pack_sets["seed"];
        let reviewed = &pack_sets["reviewed"];
        let experimental = &pack_sets["experimental-full"];
        let missing_r: Vec<&String> = seed.difference(reviewed).take(5).collect();
        let missing_e: Vec<&String> = reviewed.difference(experimental).take(5).collect();
        subset_result = if !missing_r.is_empty() {
            Err(format!("seed words missing from reviewed: {:?}", missing_r))
        } else if !missing_e.is_empty() {
            Err(format!(
                "reviewed words missing from experimental-full: {:?}",
                missing_e
            ))
        } else {
            Ok(format!(
                "seed {} ⊆ reviewed {} ⊆ experimental-full {}",
                seed.len(),
                reviewed.len(),
                experimental.len()
            ))
        };
    }
    checks.record("trust-subsets", subset_result);

    // 6. The policy's language-model set is exactly what a rebuild can generate.
    let model_set_result = match (&corpora, &policy) {
        (Ok(corpora), Ok(policy)) => {
            let expected = producible_model_ids(corpora);
            let actual = policy_model_ids(policy);
            if actual == expected {
                Ok(format!(
                    "policy models {:?} are exactly what rebuild-production generates from the registered corpora",
                    actual
                ))
            } else {
                Err(format!(
                    "pack policy references language model(s) {:?} but rebuild-production generates exactly {:?} from the registered corpora",
                    actual, expected
                ))
            }
        }
        _ => {
            Err("cannot evaluate: the corpus registry or the pack policy did not load".to_string())
        }
    };
    checks.record("policy-model-set", model_set_result);

    AuthoritativeInputs {
        checks: checks.0,
        corpora: corpora.ok(),
        policy: policy.ok(),
    }
}

/// Verifies the production state of the repository at `root` without writing anything.
pub fn verify_production_state<P: AsRef<Path>>(root: P) -> Result<ProductionStateReport, String> {
    let root = root.as_ref();
    let inputs = check_authoritative_inputs(root);
    let mut checks = Checks(inputs.checks);
    let corpora_path = root.join("data/source-registry/corpora.toml");

    // 7. Committed language model: hashes, invariants, fingerprint against the current
    //    vocabulary (all enforced by the loader), and registry provenance.
    let mut language_model = None;
    if let Some(policy) = &inputs.policy {
        let model_ids = policy_model_ids(policy);
        if model_ids.is_empty() {
            checks.skip("language-model", "no pack references a language model");
        }
        for model_id in model_ids {
            let name = format!("language-model:{}", model_id);
            match load_language_model(root, &model_id) {
                Ok(model) => {
                    let mut result = Ok(format!(
                        "artifacts and content verified; vocabulary fingerprint current ({} words)",
                        model.manifest.vocabulary_size
                    ));
                    if let Ok(registry_sha) = sha256_file(&corpora_path) {
                        if model.manifest.corpus_registry_sha256 != registry_sha {
                            result = Err(format!(
                                "model was built against corpora.toml {} but the current registry is {}; regenerate with `build-language-model --corpus-id {}`",
                                abbrev(&model.manifest.corpus_registry_sha256),
                                abbrev(&registry_sha),
                                model.manifest.corpus_id
                            ));
                        }
                    }
                    // The local corpus pipeline is an untracked cache. It is compared with the
                    // model's recorded input only when it actually contains the model's corpus;
                    // an import that skipped the corpus (CI, or a checkout without the dump) is
                    // simply what `rebuild-production` re-imports.
                    let canonical_manifest = root.join("data/imported-canonical/manifest.json");
                    if result.is_ok() && canonical_manifest.exists() {
                        match verify_canonical_manifest(root) {
                            Ok(local_import)
                                if local_import
                                    .skipped_external_corpora
                                    .contains(&model.manifest.corpus_id) =>
                            {
                                if let Ok(detail) = result.as_mut() {
                                    detail.push_str(&format!(
                                        "; local canonical import skips {} (rebuild-production re-imports it)",
                                        model.manifest.corpus_id
                                    ));
                                }
                            }
                            Ok(_) => {
                                let local = sha256_file(&canonical_manifest).unwrap_or_default();
                                if local != model.manifest.canonical_manifest_sha256 {
                                    result = Err(format!(
                                        "local canonical import ({}) includes {} but differs from the one the model was built from ({}); rerun the corpus pipeline or `rebuild-production`",
                                        abbrev(&local),
                                        model.manifest.corpus_id,
                                        abbrev(&model.manifest.canonical_manifest_sha256)
                                    ));
                                }
                                let partition_manifest =
                                    root.join("data/build/corpus-partitions/manifest.json");
                                if result.is_ok() && partition_manifest.exists() {
                                    let local =
                                        sha256_file(&partition_manifest).unwrap_or_default();
                                    if local != model.manifest.partition_manifest_sha256 {
                                        result = Err(format!(
                                            "local partition ({}) differs from the one the model was built from ({}); rerun the corpus pipeline or `rebuild-production`",
                                            abbrev(&local),
                                            abbrev(&model.manifest.partition_manifest_sha256)
                                        ));
                                    }
                                }
                            }
                            Err(e) => {
                                result = Err(format!(
                                    "local canonical import is present but does not verify: {}",
                                    e
                                ));
                            }
                        }
                    }
                    let extra: Vec<String> =
                        fs::read_dir(root.join(LANGUAGE_MODEL_DIR).join(&model_id))
                            .map(|rd| {
                                rd.filter_map(|e| e.ok())
                                    .map(|e| e.file_name().to_string_lossy().to_string())
                                    .filter(|n| {
                                        ![
                                            "vocabulary.txt",
                                            "unigrams.tsv",
                                            "bigrams.tsv",
                                            "trigrams.tsv",
                                            "manifest.json",
                                            "artifacts.sha256",
                                        ]
                                        .contains(&n.as_str())
                                    })
                                    .collect()
                            })
                            .unwrap_or_default();
                    if result.is_ok() && !extra.is_empty() {
                        result = Err(format!(
                            "unexpected files in the model directory: {:?}",
                            extra
                        ));
                    }
                    if result.is_ok() {
                        language_model = Some(LanguageModelState {
                            model_id: model_id.clone(),
                            manifest_sha256: model.manifest_sha256.clone(),
                            vocabulary_fingerprint: model.manifest.vocabulary_fingerprint.clone(),
                            vocabulary_size: model.manifest.vocabulary_size,
                        });
                    }
                    checks.record(&name, result);
                }
                Err(e) => checks.record(&name, Err(e)),
            }
        }
    } else {
        checks.skip("language-model", "pack policy did not load");
    }

    // 8. Packs: the complete artifact set is reproducible in memory and every built file
    //    equals it when the pack is built.
    let mut packs = Vec::new();
    let mut all_built = true;
    for pack_id in PACK_IDS {
        let name = format!("pack-reproducible:{}", pack_id);
        let first = assemble_pack_artifacts(pack_id, root);
        let second = assemble_pack_artifacts(pack_id, root);
        match (first, second) {
            (Ok(a), Ok(b)) => {
                let nondeterministic: Vec<&str> = PACK_ARTIFACT_FILES
                    .iter()
                    .copied()
                    .filter(|n| a.files.get(n) != b.files.get(n))
                    .collect();
                let pack_dir = root.join(format!("data/build/packs/{}", pack_id));
                let built = pack_dir.is_dir();
                let mut artifacts = Vec::new();
                let mut stale: Vec<String> = Vec::new();
                for name in PACK_ARTIFACT_FILES {
                    let in_memory_sha256 = sha256_bytes(&a.files[name]);
                    let (built_sha256, built_matches) = if built {
                        match sha256_file(&pack_dir.join(name)) {
                            Ok(sha) => {
                                let matches = sha == in_memory_sha256;
                                if !matches {
                                    stale.push(format!(
                                        "{} ({} on disk vs {} from current sources)",
                                        name,
                                        abbrev(&sha),
                                        abbrev(&in_memory_sha256)
                                    ));
                                }
                                (Some(sha), Some(matches))
                            }
                            Err(_) => {
                                stale.push(format!("{} (missing)", name));
                                (None, Some(false))
                            }
                        }
                    } else {
                        (None, None)
                    };
                    artifacts.push(PackArtifactState {
                        name: name.to_string(),
                        in_memory_sha256,
                        built_sha256,
                        built_matches,
                    });
                }
                if !built || !stale.is_empty() {
                    all_built = false;
                }
                let lexicon = &artifacts[0];
                let built_matches = if built { Some(stale.is_empty()) } else { None };
                let result = if !nondeterministic.is_empty() {
                    Err(format!(
                        "two in-memory assemblies differ in {:?}: the build is not deterministic",
                        nondeterministic
                    ))
                } else if !stale.is_empty() {
                    Err(format!(
                        "built pack {} is stale: {}; rebuild the packs",
                        pack_dir.display(),
                        stale.join(", ")
                    ))
                } else {
                    Ok(format!(
                        "{} entries, lexicon.bin {}, all {} artifacts assembled identically twice{}",
                        a.manifest.final_unique_entry_count,
                        abbrev(&lexicon.in_memory_sha256),
                        PACK_ARTIFACT_FILES.len(),
                        if built {
                            ", every built artifact matches"
                        } else {
                            ", not built locally"
                        }
                    ))
                };
                packs.push(PackReproducibility {
                    pack_id: pack_id.to_string(),
                    in_memory_sha256: lexicon.in_memory_sha256.clone(),
                    entry_count: a.manifest.final_unique_entry_count,
                    built_sha256: lexicon.built_sha256.clone(),
                    built_matches,
                    artifacts,
                });
                checks.record(&name, result);
            }
            (Err(e), _) | (_, Err(e)) => {
                all_built = false;
                checks.record(&name, Err(e));
            }
        }
    }

    // 9. Built pack manifests, when all packs are built and current.
    if all_built {
        checks.record(
            "pack-manifests",
            validate_all_pack_manifests(root)
                .map(|_| "all pack manifests, provenance and artifact hashes valid".to_string()),
        );
    } else {
        checks.skip(
            "pack-manifests",
            "not all packs are built and current under data/build/packs (run build-pack or rebuild-production)",
        );
    }

    // 10. Local corpus pipeline state, when present, must be internally consistent.
    let partition_manifest = root.join("data/build/corpus-partitions/manifest.json");
    if partition_manifest.exists() {
        let result = (|| -> Result<String, String> {
            let canonical = verify_canonical_manifest(root)?;
            let canonical_sha = sha256_file(&root.join("data/imported-canonical/manifest.json"))?;
            let manifest: PartitionBuildManifest =
                serde_json::from_slice(&fs::read(&partition_manifest).map_err(|e| e.to_string())?)
                    .map_err(|e| format!("partition manifest: {}", e))?;
            if manifest.canonical_input_manifest_sha256 != canonical_sha {
                return Err(
                    "partition was built from a different canonical import; rerun partition-corpora"
                        .to_string(),
                );
            }
            Ok(format!(
                "canonical import of {} corpora ({} skipped) and partition ({} train docs) consistent",
                canonical.corpora.len(),
                canonical.skipped_external_corpora.len(),
                manifest.train_documents
            ))
        })();
        checks.record("local-corpus-pipeline", result);
    } else {
        checks.skip(
            "local-corpus-pipeline",
            "no local canonical import / partition (not needed unless regenerating the model)",
        );
    }

    let ok = checks.0.iter().all(|c| c.status != CheckStatus::Fail);
    Ok(ProductionStateReport {
        schema_version: PRODUCTION_STATE_SCHEMA_VERSION.to_string(),
        ok,
        checks: checks.0,
        packs,
        language_model,
    })
}

pub fn render_checks_text(checks: &[Check]) -> String {
    let mut s = String::new();
    for c in checks {
        let mark = match c.status {
            CheckStatus::Pass => "PASS",
            CheckStatus::Fail => "FAIL",
            CheckStatus::Skipped => "SKIP",
        };
        s.push_str(&format!("{:<4} {:<32} {}\n", mark, c.name, c.detail));
    }
    s
}

pub fn render_state_text(r: &ProductionStateReport) -> String {
    let mut s = render_checks_text(&r.checks);
    s.push_str(&format!(
        "\nproduction state: {}\n",
        if r.ok { "OK" } else { "NOT OK" }
    ));
    s
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HashChange {
    pub artifact: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before: Option<String>,
    pub after: String,
    pub changed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RebuildReport {
    pub schema_version: String,
    pub steps: Vec<String>,
    pub hashes: Vec<HashChange>,
    pub state: ProductionStateReport,
}

fn hash_change(artifact: &str, before: Option<String>, after: String) -> HashChange {
    let changed = before.as_deref() != Some(after.as_str());
    HashChange {
        artifact: artifact.to_string(),
        before,
        after,
        changed,
    }
}

/// Every artifact a rebuild regenerates, with its current hash (None when absent).
fn rebuilt_artifact_hashes(root: &Path, model_id: &str) -> Vec<(String, Option<String>)> {
    let mut out = vec![(
        format!("{}/{}/manifest.json", LANGUAGE_MODEL_DIR, model_id),
        language_model_manifest_sha256(root, model_id).ok(),
    )];
    for pack_id in PACK_IDS {
        for name in PACK_ARTIFACT_FILES {
            let rel = format!("data/build/packs/{}/{}", pack_id, name);
            out.push((rel.clone(), sha256_file(&root.join(&rel)).ok()));
        }
    }
    out
}

/// Rebuilds the corpus-derived statistics, the committed language model and the packs from
/// the committed human decisions, then verifies.
///
/// Before anything is mutated, the authoritative inputs are verified
/// (`check_authoritative_inputs`); any failure there returns an error and changes nothing.
/// The Kuwiki corpus must be registered; when it is not present locally the command fails
/// without changes unless `acquire` is true, the only path that downloads. Never creates,
/// changes or promotes a vocabulary decision.
pub fn rebuild_production<P: AsRef<Path>>(root: P, acquire: bool) -> Result<RebuildReport, String> {
    let root = root.as_ref();
    let mut steps = Vec::new();

    // Read-only preflight of everything a rebuild cannot repair.
    let inputs = check_authoritative_inputs(root);
    if !inputs.ok() {
        return Err(format!(
            "authoritative inputs failed verification; nothing was changed:\n{}",
            render_checks_text(&inputs.checks)
        ));
    }
    steps.push("preflight: authoritative inputs verified".to_string());
    let registry = inputs
        .corpora
        .ok_or_else(|| "corpus registry did not load".to_string())?;
    let kuwiki = registry.find_corpus(KUWIKI_CORPUS_ID).ok_or_else(|| {
        format!(
            "corpus '{}' is not registered in corpora.toml; nothing to rebuild the model from (nothing was changed)",
            KUWIKI_CORPUS_ID
        )
    })?;
    // The preflight proved the policy's model set is exactly this one id.
    let model_id = format!("{}-{}", KUWIKI_CORPUS_ID, kuwiki.version);
    if !kuwiki.files_present(root) {
        if acquire {
            steps.push("acquire-corpus kuwiki (explicitly requested)".to_string());
            acquire_corpus(KUWIKI_CORPUS_ID, root)?;
        } else {
            return Err(format!(
                "the Kuwiki corpus is not present locally ({}); run `acquire-corpus {}` or pass --acquire to download and verify it (nothing was changed)",
                kuwiki
                    .files
                    .iter()
                    .map(|f| f.path.clone())
                    .collect::<Vec<_>>()
                    .join(", "),
                KUWIKI_CORPUS_ID
            ));
        }
    }

    let before = rebuilt_artifact_hashes(root, &model_id);

    // Corpus-derived statistics, only when the local import is missing, stale or incomplete.
    let canonical_needs_import = match verify_canonical_manifest(root) {
        Ok(m) => m
            .skipped_external_corpora
            .contains(&KUWIKI_CORPUS_ID.to_string()),
        Err(_) => true,
    };
    if canonical_needs_import {
        steps.push(
            "import-all-corpora (canonical import missing, stale or without kuwiki)".to_string(),
        );
        import_all_corpora(root)?;
    }
    let canonical_sha = sha256_file(&root.join("data/imported-canonical/manifest.json"))?;
    let partition_manifest = root.join("data/build/corpus-partitions/manifest.json");
    let partition_stale = match fs::read(&partition_manifest) {
        Ok(bytes) => serde_json::from_slice::<PartitionBuildManifest>(&bytes)
            .map(|m| m.canonical_input_manifest_sha256 != canonical_sha)
            .unwrap_or(true),
        Err(_) => true,
    };
    if partition_stale {
        steps
            .push("partition-corpora (partition missing or built from another import)".to_string());
        partition_corpora(root)?;
    }

    // Language model and packs (deterministic; unchanged inputs give unchanged outputs).
    let lm = PRODUCTION_LANGUAGE_MODEL_BUILD;
    steps.push(format!(
        "build-language-model --corpus-id {} --bigram-min-count {} --trigram-min-count {}",
        KUWIKI_CORPUS_ID, lm.bigram_min_count, lm.trigram_min_count
    ));
    build_language_model(
        root,
        KUWIKI_CORPUS_ID,
        lm.bigram_min_count,
        lm.trigram_min_count,
    )?;
    for pack_id in PACK_IDS {
        steps.push(format!("build-pack {}", pack_id));
        build_pack(pack_id, root)?;
    }
    steps.push("validate-pack-manifest".to_string());
    validate_all_pack_manifests(root)?;
    steps.push("verify-production-state".to_string());
    let state = verify_production_state(root)?;

    let mut hashes = Vec::new();
    for ((artifact, before), (_, after)) in before
        .into_iter()
        .zip(rebuilt_artifact_hashes(root, &model_id))
    {
        let after = after.ok_or_else(|| format!("{} is missing after the rebuild", artifact))?;
        hashes.push(hash_change(&artifact, before, after));
    }
    if !state.ok {
        return Err(format!(
            "rebuild completed but verification failed:\n{}",
            render_state_text(&state)
        ));
    }
    Ok(RebuildReport {
        schema_version: PRODUCTION_STATE_SCHEMA_VERSION.to_string(),
        steps,
        hashes,
        state,
    })
}

pub fn render_rebuild_text(r: &RebuildReport) -> String {
    let mut s = String::from("steps:\n");
    for step in &r.steps {
        s.push_str(&format!("  {}\n", step));
    }
    s.push_str("artifacts:\n");
    for h in &r.hashes {
        s.push_str(&format!(
            "  {:<58} {} {}\n",
            h.artifact,
            abbrev(&h.after),
            if h.changed {
                match &h.before {
                    Some(b) => format!("(changed from {})", abbrev(b)),
                    None => "(new)".to_string(),
                }
            } else {
                "(unchanged)".to_string()
            }
        ));
    }
    s.push('\n');
    s.push_str(&render_state_text(&r.state));
    s
}
