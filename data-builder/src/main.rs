use clap::{Parser, Subcommand};
use data_builder_lib::{
    calculate_sha256, compile_binary_pack, evaluate_lexicon_impact, generate_and_save_report,
    import_hunspell_dic, merge_and_deduplicate, normalize_text, validate_entry, write_artifacts,
    BuildReport, BuilderConfig, ReleaseManifest, SourceLexiconEntry, SourceRegistry,
};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "data-builder",
    author = "Kurmancî Language Platform Contributors",
    version = "0.1.0",
    about = "Kurmancî Language Platform Data Compiler Subsystem"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Executes the full reproducible build pipeline (source -> normalized -> validated -> merged -> compiled binary)
    Build {
        #[arg(short, long, default_value = "data-builder/config/builder.toml")]
        config: PathBuf,
    },
    /// Verifies all registered preserved source files in sources.toml against SHA-256 checksums and provenance limits
    VerifySources {
        #[arg(short, long, default_value = "data/source-registry/sources.toml")]
        registry: PathBuf,
    },
    /// Atomically acquires source files for a source ID from commit-pinned URLs with SHA-256 verification
    AcquireSource {
        #[arg(index = 1)]
        source_id: String,
        #[arg(short, long, default_value = "data/source-registry/sources.toml")]
        registry: PathBuf,
    },
    /// Deterministically parses and imports a registered Hunspell .dic source
    ImportHunspell {
        #[arg(index = 1)]
        source_id: String,
    },
    /// Evaluates engine benchmark performance metrics between baseline manual seed and imported Hunspell entries
    EvaluateLexicon {
        #[arg(
            short,
            long,
            default_value = "data/imported/kurdish-hunspell-kmr/lexicon.jsonl"
        )]
        imported: PathBuf,
    },
}

fn main() {
    let cli = Cli::parse();
    let command = cli.command.unwrap_or(Commands::Build {
        config: PathBuf::from("data-builder/config/builder.toml"),
    });

    match command {
        Commands::Build { config } => {
            let builder_cfg = if config.exists() {
                BuilderConfig::load_from_file(&config)
                    .unwrap_or_else(|e| panic!("Error loading config {:?}: {}", config, e))
            } else {
                BuilderConfig::default()
            };

            println!("=== Kurmancî Data-Builder Compiler Pipeline ===");
            println!("Configuration: {:?}", builder_cfg.build);

            let source_path = &builder_cfg.build.source_file;
            let file = File::open(source_path)
                .unwrap_or_else(|_| panic!("Failed to open source file '{}'", source_path));
            let reader = BufReader::new(file);

            let mut raw_entries = Vec::new();
            let mut total_source_entries = 0;

            for (line_idx, line_res) in reader.lines().enumerate() {
                let line = line_res.expect("Error reading line");
                if line.trim().is_empty() {
                    continue;
                }
                total_source_entries += 1;

                let mut entry: SourceLexiconEntry =
                    serde_json::from_str(&line).unwrap_or_else(|e| {
                        panic!("Line {}: Malformed JSON schema: {}", line_idx + 1, e)
                    });

                // 1. Unicode Normalization
                entry.normalized = normalize_text(&entry.normalized);
                entry.word = normalize_text(&entry.word);

                // 2. Validation
                validate_entry(&entry, line_idx + 1)
                    .unwrap_or_else(|e| panic!("Validation failed: {}", e));

                raw_entries.push(entry);
            }

            let validated_entries_count = raw_entries.len();
            println!(
                "  [1/4] Loaded & validated {} source entries.",
                validated_entries_count
            );

            // 3. Merge & Deduplicate
            let merged_entries = merge_and_deduplicate(raw_entries);
            let unique_count = merged_entries.len();
            println!(
                "  [2/4] Merged & deduplicated into {} unique entries.",
                unique_count
            );

            // 4. Binary Compilation & Checksum Calculation
            let binary_bytes =
                compile_binary_pack(&merged_entries).expect("Binary compilation failed");
            let checksum = calculate_sha256(&binary_bytes);
            println!(
                "  [3/4] Compiled binary pack ({:.2} KB, SHA-256: {}).",
                binary_bytes.len() as f64 / 1024.0,
                checksum
            );

            // 5. Manifest & Report Generation
            let manifest = ReleaseManifest {
                project: "kurmanci-language-platform".to_string(),
                language: builder_cfg.build.language_tag.clone(),
                data_version: builder_cfg.build.data_version.clone(),
                format_version: builder_cfg.build.format_version,
                entry_count: unique_count,
                checksum_sha256: checksum.clone(),
                build_configuration_hash: calculate_sha256(
                    format!("{:?}", builder_cfg.build).as_bytes(),
                ),
                reproducible: true,
            };

            write_artifacts(&builder_cfg.build.build_dir, &binary_bytes, &manifest)
                .expect("Failed to write build artifacts");

            let report = BuildReport {
                timestamp: "2026-07-30T00:00:00Z".to_string(),
                total_source_entries,
                validated_entries: validated_entries_count,
                unique_lexicon_entries: unique_count,
                binary_pack_size_bytes: binary_bytes.len(),
                checksum_sha256: checksum,
                status: "SUCCESS".to_string(),
            };

            generate_and_save_report(&builder_cfg.build.reports_dir, &report)
                .expect("Failed to write build report");

            println!("  [4/4] Successfully generated manifest & report.");
            println!("⚡ BUILD SUCCESSFUL!");
        }
        Commands::VerifySources { registry } => {
            println!("=== Kurmancî Source Registry Integrity Verification ===");
            let reg = SourceRegistry::load_from_file(&registry)
                .unwrap_or_else(|e| panic!("Failed to load source registry {:?}: {}", registry, e));

            reg.verify_preserved_files(".")
                .unwrap_or_else(|e| panic!("Source verification failed: {}", e));

            println!(
                "⚡ Source Registry Verification PASSED! Verified {} registered sources.",
                reg.sources.len()
            );
        }
        Commands::AcquireSource {
            source_id,
            registry,
        } => {
            println!("=== Kurmancî Deterministic Source Acquisition ===");
            println!("Acquiring source ID '{}' from {:?}", source_id, registry);

            let reg = SourceRegistry::load_from_file(&registry)
                .unwrap_or_else(|e| panic!("Failed to load source registry {:?}: {}", registry, e));

            reg.acquire_source(&source_id, ".", None)
                .unwrap_or_else(|e| panic!("Source acquisition failed: {}", e));

            println!(
                "⚡ Source Acquisition SUCCESSFUL! Source '{}' acquired and verified.",
                source_id
            );
        }
        Commands::ImportHunspell { source_id } => {
            println!("=== Kurmancî Deterministic Hunspell Importer ===");
            let summary = import_hunspell_dic(&source_id, ".")
                .unwrap_or_else(|e| panic!("Hunspell import failed: {}", e));

            println!("  Source ID:               {}", summary.source_id);
            println!(
                "  Declared Entries Count:  {:?}",
                summary.declared_entry_count
            );
            println!(
                "  Physical Input Lines:    {}",
                summary.physical_input_lines
            );
            println!("  Parsed Entries:          {}", summary.parsed_entries);
            println!("  Accepted Entries:        {}", summary.accepted_entries);
            println!("  Rejected Entries:        {}", summary.rejected_entries);
            println!(
                "  Duplicate Surface Forms: {}",
                summary.duplicate_surface_forms
            );
            println!(
                "  Conflicting Flag Sets:   {}",
                summary.conflicting_flag_sets
            );
            println!(
                "  Output SHA-256 Checksum: {}",
                summary.output_checksum_sha256
            );
            println!("⚡ IMPORT SUCCESSFUL!");
        }
        Commands::EvaluateLexicon { imported } => {
            println!("=== Kurmancî Lexicon Impact Benchmark Evaluation ===");
            let eval_report = evaluate_lexicon_impact(&imported, &PathBuf::from("."))
                .unwrap_or_else(|e| panic!("Lexicon evaluation failed: {}", e));

            println!("\nBaseline (Manual Seed):");
            println!(
                "  Entries:      {}",
                eval_report.baseline_manual_seed.entry_count
            );
            println!(
                "  Binary Size:  {:.2} KB",
                eval_report.baseline_manual_seed.binary_pack_size_bytes as f64 / 1024.0
            );
            println!(
                "  Coverage:     {:.1}%",
                eval_report.baseline_manual_seed.known_word_coverage_percent
            );
            println!(
                "  Top-1 Acc:    {:.1}%",
                eval_report
                    .baseline_manual_seed
                    .correction_top_1_accuracy_percent
            );
            println!(
                "  Top-K Acc:    {:.1}%",
                eval_report
                    .baseline_manual_seed
                    .correction_top_k_accuracy_percent
            );
            println!(
                "  Load Time:    {} µs",
                eval_report.baseline_manual_seed.load_time_us
            );
            println!(
                "  Query Lat:    {:.2} µs",
                eval_report.baseline_manual_seed.avg_query_latency_us
            );

            println!("\nCombined (Seed + Imported Hunspell):");
            println!(
                "  Entries:      {}",
                eval_report.combined_with_imported.entry_count
            );
            println!(
                "  Binary Size:  {:.2} KB",
                eval_report.combined_with_imported.binary_pack_size_bytes as f64 / 1024.0
            );
            println!(
                "  Coverage:     {:.1}%",
                eval_report
                    .combined_with_imported
                    .known_word_coverage_percent
            );
            println!(
                "  Top-1 Acc:    {:.1}%",
                eval_report
                    .combined_with_imported
                    .correction_top_1_accuracy_percent
            );
            println!(
                "  Top-K Acc:    {:.1}%",
                eval_report
                    .combined_with_imported
                    .correction_top_k_accuracy_percent
            );
            println!(
                "  Load Time:    {} µs",
                eval_report.combined_with_imported.load_time_us
            );
            println!(
                "  Query Lat:    {:.2} µs",
                eval_report.combined_with_imported.avg_query_latency_us
            );

            println!("\nQuality Note: {}", eval_report.quality_note);
            println!("⚡ EVALUATION COMPLETED!");
        }
    }
}
