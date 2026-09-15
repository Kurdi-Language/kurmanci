//! Engineering benchmark for compiled Kurmancî language packs.
//!
//! Loads one pack through the public `KurmanciEngine` API and reports:
//!
//! - pack bytes, load time, and heap allocated by the load (peak during load versus the
//!   steady state kept by the engine, so temporary load allocations are visible);
//! - the engine's own per-structure memory attribution (`MemoryAttribution`);
//! - process RSS (current and peak) after load;
//! - query latency percentiles for known-word, suggest, correct, complete and predict
//!   operations, with the contexts discovered from the pack itself so that bigram, trigram,
//!   backoff and zero-result paths are all exercised;
//! - heap growth across the query loop (steady-state check).
//!
//! It never modifies packs and asserts no thresholds: numbers are for reporting. Run in
//! release mode for meaningful timings:
//!
//! `cargo run --release -p kurmanci-bench -- data/build/packs/reviewed/lexicon.bin [--json]`

use clap::Parser;
use kurmanci_engine::{
    CompletionOptions, CorrectionOptions, KurmanciEngine, MemoryAttribution, PredictionOptions,
    PredictionSource, SuggestOptions,
};
use serde::Serialize;
use std::alloc::{GlobalAlloc, Layout, System};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

/// Global allocator wrapper counting live bytes, peak live bytes and allocation calls.
struct CountingAllocator;

static LIVE_BYTES: AtomicUsize = AtomicUsize::new(0);
static PEAK_BYTES: AtomicUsize = AtomicUsize::new(0);
static ALLOC_CALLS: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let p = System.alloc(layout);
        if !p.is_null() {
            record_alloc(layout.size());
        }
        p
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout);
        LIVE_BYTES.fetch_sub(layout.size(), Ordering::Relaxed);
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let p = System.realloc(ptr, layout, new_size);
        if !p.is_null() {
            LIVE_BYTES.fetch_sub(layout.size(), Ordering::Relaxed);
            record_alloc(new_size);
        }
        p
    }
}

fn record_alloc(size: usize) {
    ALLOC_CALLS.fetch_add(1, Ordering::Relaxed);
    let live = LIVE_BYTES.fetch_add(size, Ordering::Relaxed) + size;
    PEAK_BYTES.fetch_max(live, Ordering::Relaxed);
}

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

fn live_bytes() -> usize {
    LIVE_BYTES.load(Ordering::Relaxed)
}

fn reset_peak() {
    PEAK_BYTES.store(live_bytes(), Ordering::Relaxed);
}

fn peak_bytes() -> usize {
    PEAK_BYTES.load(Ordering::Relaxed)
}

fn alloc_calls() -> usize {
    ALLOC_CALLS.load(Ordering::Relaxed)
}

/// Peak resident set size of this process in bytes (`getrusage`).
fn peak_rss_bytes() -> u64 {
    let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
    unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) };
    // macOS reports ru_maxrss in bytes; Linux in kilobytes.
    if cfg!(target_os = "macos") {
        usage.ru_maxrss as u64
    } else {
        (usage.ru_maxrss as u64) * 1024
    }
}

/// Current resident set size of this process in bytes, if the platform exposes it.
#[cfg(target_os = "macos")]
fn current_rss_bytes() -> Option<u64> {
    let mut info: libc::proc_taskinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_taskinfo>() as libc::c_int;
    let written = unsafe {
        libc::proc_pidinfo(
            std::process::id() as libc::c_int,
            libc::PROC_PIDTASKINFO,
            0,
            &mut info as *mut _ as *mut libc::c_void,
            size,
        )
    };
    if written == size {
        Some(info.pti_resident_size)
    } else {
        None
    }
}

#[cfg(target_os = "linux")]
fn current_rss_bytes() -> Option<u64> {
    let statm = std::fs::read_to_string("/proc/self/statm").ok()?;
    let resident_pages: u64 = statm.split_whitespace().nth(1)?.parse().ok()?;
    let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    if page > 0 {
        Some(resident_pages * page as u64)
    } else {
        None
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn current_rss_bytes() -> Option<u64> {
    None
}

#[derive(Parser)]
#[command(
    name = "kurmanci-bench",
    about = "Memory attribution and latency benchmark for a compiled Kurmancî language pack"
)]
struct Cli {
    /// Compiled binary language pack (lexicon.bin)
    pack: PathBuf,
    /// Query iterations per operation
    #[arg(long, default_value_t = 500)]
    iterations: usize,
    /// Number of cold loads used for the load-time distribution
    #[arg(long, default_value_t = 5)]
    loads: usize,
    /// Words probed (in order) to discover bigram and trigram contexts present in the pack
    #[arg(long, num_args = 1.., default_values_t = ["ez", "li", "di", "ji", "ku", "bo", "bi"].map(String::from))]
    probe_words: Vec<String>,
    /// Emit the report as JSON
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Clone, Serialize)]
struct LoadReport {
    pack_bytes: usize,
    entry_count: usize,
    load_ms_median: f64,
    load_ms_min: f64,
    load_ms_max: f64,
    /// Heap bytes still allocated after the load (the engine's steady-state footprint plus
    /// the pack bytes buffer, reported separately).
    heap_after_load_bytes: usize,
    /// Highest live heap during the load, above the pre-load baseline (the pack bytes buffer
    /// is allocated before the baseline and therefore excluded).
    heap_peak_during_load_bytes: usize,
    /// Peak minus (engine steady state + pack buffer): temporary allocations that the load
    /// frees before returning.
    heap_temporary_load_bytes: usize,
    /// Allocation calls made by the load.
    load_allocation_calls: usize,
    /// Steady-state heap attributed to the engine (heap after load minus the pack buffer).
    engine_heap_bytes: usize,
    engine_heap_bytes_per_entry: f64,
    /// Ratio of steady-state engine heap to pack bytes.
    heap_to_pack_ratio: f64,
    rss_after_load_bytes: Option<u64>,
    peak_rss_after_load_bytes: u64,
    attribution: MemoryAttribution,
    /// Share of the engine's steady-state heap that the attribution model explains.
    attribution_coverage: f64,
}

#[derive(Debug, Clone, Serialize)]
struct LatencyReport {
    operation: String,
    input: String,
    iterations: usize,
    p50_us: f64,
    p95_us: f64,
    p99_us: f64,
    max_us: f64,
    /// Number of iterations returning a non-empty result (all or none for a fixed input).
    non_empty: usize,
    /// Result count of the last iteration.
    result_count: usize,
    /// Prediction source of the last iteration, for predict operations.
    #[serde(skip_serializing_if = "Option::is_none")]
    prediction_source: Option<String>,
    /// Highest transient heap one call of this operation allocated above the steady state
    /// (freed again before the call returns; this is what drives peak RSS during queries).
    transient_peak_bytes: usize,
}

#[derive(Debug, Clone, Serialize)]
struct Report {
    pack: String,
    build_profile: &'static str,
    host: String,
    /// Additional cold loads performed for the load-time distribution.
    reloads: usize,
    load: LoadReport,
    latencies: Vec<LatencyReport>,
    /// Live heap growth across the whole query loop (should stay near zero for an immutable
    /// engine; the engine keeps no caches).
    heap_growth_during_queries_bytes: i64,
    /// RSS after the query loop, with one engine alive.
    rss_after_queries_bytes: Option<u64>,
    /// Peak RSS up to the end of the query loop (one engine alive throughout).
    peak_rss_after_queries_bytes: u64,
    /// Peak RSS after the additional cold loads used for the load-time distribution. Those
    /// loads briefly hold two engines, so this figure is a benchmark artifact, not the
    /// footprint of one loaded engine.
    peak_rss_after_reloads_bytes: u64,
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = ((sorted.len() as f64 - 1.0) * p).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

fn measure<F: FnMut() -> (usize, Option<PredictionSource>)>(
    operation: &str,
    input: &str,
    iterations: usize,
    mut f: F,
) -> LatencyReport {
    let mut samples = Vec::with_capacity(iterations);
    let mut non_empty = 0usize;
    let mut result_count = 0usize;
    let mut source = None;
    let mut transient_peak = 0usize;
    for _ in 0..iterations {
        let live_before = live_bytes();
        reset_peak();
        let t = Instant::now();
        let (count, src) = f();
        samples.push(t.elapsed().as_secs_f64() * 1e6);
        transient_peak = transient_peak.max(peak_bytes().saturating_sub(live_before));
        if count > 0 {
            non_empty += 1;
        }
        result_count = count;
        source = src;
    }
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
    LatencyReport {
        operation: operation.to_string(),
        input: input.to_string(),
        iterations,
        p50_us: percentile(&samples, 0.5),
        p95_us: percentile(&samples, 0.95),
        p99_us: percentile(&samples, 0.99),
        max_us: samples.last().copied().unwrap_or(0.0),
        non_empty,
        result_count,
        prediction_source: source.map(|s| format!("{:?}", s)),
        transient_peak_bytes: transient_peak,
    }
}

/// Finds, from the probe words, one bigram context, one trigram context (a probe word
/// followed by one of its own bigram predictions) and one backoff context (an unknown word
/// followed by the bigram context word). Returns what the pack actually contains.
fn discover_contexts(
    engine: &KurmanciEngine,
    probe_words: &[String],
) -> (Option<String>, Option<(String, String)>) {
    let opts = PredictionOptions { limit: 16 };
    let mut bigram_word = None;
    let mut trigram_ctx = None;
    for w in probe_words {
        let preds = engine.predict_next(&[w.as_str()], opts);
        if preds.is_empty() {
            continue;
        }
        if bigram_word.is_none() {
            bigram_word = Some(w.clone());
        }
        for p in &preds {
            let two = engine.predict_next(&[w.as_str(), p.text.as_str()], opts);
            if two
                .first()
                .map(|x| x.source == PredictionSource::Trigram)
                .unwrap_or(false)
            {
                trigram_ctx = Some((w.clone(), p.text.clone()));
                break;
            }
        }
        if bigram_word.is_some() && trigram_ctx.is_some() {
            break;
        }
    }
    (bigram_word, trigram_ctx)
}

fn host_description() -> String {
    let arch = std::env::consts::ARCH;
    let os = std::env::consts::OS;
    format!("{} {}", os, arch)
}

fn run(cli: &Cli) -> Result<Report, String> {
    let bytes = std::fs::read(&cli.pack)
        .map_err(|e| format!("Failed to read pack {:?}: {}", cli.pack, e))?;
    let pack_bytes = bytes.len();

    // Attributed load: one load with allocation counters observed.
    let heap_before = live_bytes();
    let calls_before = alloc_calls();
    reset_peak();
    let t = Instant::now();
    let engine = KurmanciEngine::from_pack_bytes(&bytes)
        .map_err(|e| format!("Failed to load pack: {}", e))?;
    let first_load_ms = t.elapsed().as_secs_f64() * 1000.0;
    let heap_after = live_bytes();
    let heap_peak = peak_bytes();
    let load_calls = alloc_calls() - calls_before;
    let rss_after_load = current_rss_bytes();
    let peak_rss_after_load = peak_rss_bytes();
    let attribution = engine.memory_attribution();

    // Queries.
    let n = cli.iterations.max(1);
    let (bigram_word, trigram_ctx) = discover_contexts(&engine, &cli.probe_words);
    let heap_query_start = live_bytes() as i64;
    let mut latencies = Vec::new();
    let sugg = |count: usize| (count, None);

    latencies.push(measure("known_hit", "welat", n, || {
        sugg(usize::from(engine.is_known_word("welat")))
    }));
    latencies.push(measure("known_miss", "xyzqwv", n, || {
        sugg(usize::from(engine.is_known_word("xyzqwv")))
    }));
    latencies.push(measure("suggest_exact", "welat", n, || {
        sugg(engine.suggest("welat", SuggestOptions { limit: 5 }).len())
    }));
    latencies.push(measure("suggest_diacritic", "rojbas", n, || {
        sugg(engine.suggest("rojbas", SuggestOptions { limit: 5 }).len())
    }));
    latencies.push(measure("correct_typo", "spaz", n, || {
        sugg(engine.correct("spaz", CorrectionOptions { limit: 5 }).len())
    }));
    latencies.push(measure("complete_short", "ro", n, || {
        sugg(engine.complete("ro", CompletionOptions { limit: 5 }).len())
    }));
    latencies.push(measure("complete_long", "rojb", n, || {
        sugg(
            engine
                .complete("rojb", CompletionOptions { limit: 5 })
                .len(),
        )
    }));

    let popts = PredictionOptions { limit: 5 };
    let source_of = |preds: &[kurmanci_engine::Prediction]| preds.first().map(|p| p.source);
    if let Some(w) = &bigram_word {
        latencies.push(measure("predict_bigram", w, n, || {
            let p = engine.predict_next(&[w.as_str()], popts);
            (p.len(), source_of(&p))
        }));
        let backoff_input = format!("xyzqwv {}", w);
        latencies.push(measure("predict_backoff", &backoff_input, n, || {
            let p = engine.predict_next(&["xyzqwv", w.as_str()], popts);
            (p.len(), source_of(&p))
        }));
    }
    if let Some((w2, w1)) = &trigram_ctx {
        let input = format!("{} {}", w2, w1);
        latencies.push(measure("predict_trigram", &input, n, || {
            let p = engine.predict_next(&[w2.as_str(), w1.as_str()], popts);
            (p.len(), source_of(&p))
        }));
    }
    latencies.push(measure("predict_zero", "xyzqwv zzqxw", n, || {
        let p = engine.predict_next(&["xyzqwv", "zzqxw"], popts);
        (p.len(), source_of(&p))
    }));
    let heap_growth = live_bytes() as i64 - heap_query_start;
    let rss_after_queries = current_rss_bytes();
    let peak_rss_after_queries = peak_rss_bytes();

    // Load-time distribution over additional cold loads, run last: each one builds a second
    // engine while the first is alive, and the allocator keeps the freed pages resident, so
    // any RSS read after this loop would overstate the footprint of one engine.
    let mut loads = vec![first_load_ms];
    for _ in 1..cli.loads.max(1) {
        let t = Instant::now();
        let e = KurmanciEngine::from_pack_bytes(&bytes)
            .map_err(|e| format!("Failed to load pack: {}", e))?;
        loads.push(t.elapsed().as_secs_f64() * 1000.0);
        drop(e);
    }
    loads.sort_by(|a, b| a.partial_cmp(b).unwrap());

    let engine_heap = heap_after.saturating_sub(heap_before);
    let entry_count = engine.len();
    let load = LoadReport {
        pack_bytes,
        entry_count,
        load_ms_median: percentile(&loads, 0.5),
        load_ms_min: loads[0],
        load_ms_max: loads[loads.len() - 1],
        heap_after_load_bytes: heap_after,
        heap_peak_during_load_bytes: heap_peak.saturating_sub(heap_before),
        heap_temporary_load_bytes: heap_peak.saturating_sub(heap_after),
        load_allocation_calls: load_calls,
        engine_heap_bytes: engine_heap,
        engine_heap_bytes_per_entry: if entry_count == 0 {
            0.0
        } else {
            engine_heap as f64 / entry_count as f64
        },
        heap_to_pack_ratio: if pack_bytes == 0 {
            0.0
        } else {
            engine_heap as f64 / pack_bytes as f64
        },
        rss_after_load_bytes: rss_after_load,
        peak_rss_after_load_bytes: peak_rss_after_load,
        attribution_coverage: if engine_heap == 0 {
            0.0
        } else {
            attribution.total.bytes as f64 / engine_heap as f64
        },
        attribution,
    };

    Ok(Report {
        pack: cli.pack.display().to_string(),
        build_profile: if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        },
        host: host_description(),
        reloads: cli.loads.max(1) - 1,
        load,
        latencies,
        heap_growth_during_queries_bytes: heap_growth,
        rss_after_queries_bytes: rss_after_queries,
        peak_rss_after_queries_bytes: peak_rss_after_queries,
        peak_rss_after_reloads_bytes: peak_rss_bytes(),
    })
}

fn mb(bytes: usize) -> f64 {
    bytes as f64 / 1e6
}

fn print_text(r: &Report) {
    let l = &r.load;
    let a = &l.attribution;
    println!(
        "pack: {}  ({} profile, {})",
        r.pack, r.build_profile, r.host
    );
    println!(
        "entries: {}  pack_bytes: {}  load_ms: median {:.2} min {:.2} max {:.2}",
        l.entry_count, l.pack_bytes, l.load_ms_median, l.load_ms_min, l.load_ms_max
    );
    println!(
        "heap: engine steady-state {:.2} MB ({:.0} B/entry, {:.2}x pack)  peak during load {:.2} MB  temporary {:.2} MB  allocation calls {}  (excludes the {:.2} MB pack buffer)",
        mb(l.engine_heap_bytes),
        l.engine_heap_bytes_per_entry,
        l.heap_to_pack_ratio,
        mb(l.heap_peak_during_load_bytes),
        mb(l.heap_temporary_load_bytes),
        l.load_allocation_calls,
        mb(l.pack_bytes)
    );
    let fmt_rss = |b: Option<u64>| {
        b.map(|b| format!("{:.1} MB", b as f64 / 1e6))
            .unwrap_or_else(|| "n/a".to_string())
    };
    println!(
        "rss: after load {} (peak {:.1} MB)  after queries {} (peak {:.1} MB)  peak after {} reload(s) for timing {:.1} MB (two engines briefly alive; benchmark artifact)",
        fmt_rss(l.rss_after_load_bytes),
        l.peak_rss_after_load_bytes as f64 / 1e6,
        fmt_rss(r.rss_after_queries_bytes),
        r.peak_rss_after_queries_bytes as f64 / 1e6,
        r.reloads,
        r.peak_rss_after_reloads_bytes as f64 / 1e6
    );
    println!();
    println!(
        "attribution (model explains {:.0}% of steady-state heap; record sizes: entry {} B, trie node {} B)",
        l.attribution_coverage * 100.0,
        a.entry_record_bytes,
        a.trie_node_record_bytes
    );
    let rows: [(&str, &kurmanci_engine::StructureMemory, String); 10] = [
        (
            "lexicon records",
            &a.lexicon_records,
            format!("{} entries", a.entry_count),
        ),
        (
            "lexicon strings",
            &a.lexicon_strings,
            format!("{} chars", a.lexicon_string_chars),
        ),
        (
            "lexicon regions/sources",
            &a.lexicon_regions_sources,
            String::new(),
        ),
        (
            "trie child tables",
            &a.trie_child_tables,
            format!("{} nodes", a.trie_nodes),
        ),
        (
            "trie word copies",
            &a.trie_word_copies,
            format!("{} terminals", a.trie_terminal_nodes),
        ),
        (
            "bigram table",
            &a.bigram_table,
            format!("{} contexts", a.bigram_contexts),
        ),
        (
            "bigram lists",
            &a.bigram_lists,
            format!("{} predictions", a.bigram_predictions),
        ),
        (
            "trigram table",
            &a.trigram_table,
            format!("{} contexts", a.trigram_contexts),
        ),
        (
            "trigram lists",
            &a.trigram_lists,
            format!("{} predictions", a.trigram_predictions),
        ),
        ("typo map", &a.typo_map, String::new()),
    ];
    println!(
        "  {:<26}{:>12}{:>8}{:>14}  count",
        "structure", "MB", "share", "allocations"
    );
    for (name, part, count) in rows {
        let share = if a.total.bytes == 0 {
            0.0
        } else {
            part.bytes as f64 * 100.0 / a.total.bytes as f64
        };
        println!(
            "  {:<26}{:>12.3}{:>7.1}%{:>14}  {}",
            name,
            mb(part.bytes),
            share,
            part.allocations,
            count
        );
    }
    println!(
        "  {:<26}{:>12.3}{:>8}{:>14}",
        "total attributed",
        mb(a.total.bytes),
        "100%",
        a.total.allocations
    );
    println!();
    println!(
        "  {:<18}{:<16}{:>10}{:>10}{:>10}{:>10}{:>14}  result",
        "operation", "input", "p50 us", "p95 us", "p99 us", "max us", "transient MB"
    );
    for lat in &r.latencies {
        println!(
            "  {:<18}{:<16}{:>10.2}{:>10.2}{:>10.2}{:>10.2}{:>14.3}  {} results{}{}",
            lat.operation,
            lat.input,
            lat.p50_us,
            lat.p95_us,
            lat.p99_us,
            lat.max_us,
            mb(lat.transient_peak_bytes),
            lat.result_count,
            if lat.non_empty == 0 { " (empty)" } else { "" },
            lat.prediction_source
                .as_ref()
                .map(|s| format!(" [{}]", s))
                .unwrap_or_default()
        );
    }
    println!(
        "heap growth during queries: {} bytes",
        r.heap_growth_during_queries_bytes
    );
}

fn main() {
    let cli = Cli::parse();
    match run(&cli) {
        Ok(report) => {
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&report).unwrap());
            } else {
                print_text(&report);
            }
        }
        Err(e) => {
            eprintln!("error: {}", e);
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentile_is_index_based() {
        let s = [1.0, 2.0, 3.0, 4.0, 5.0];
        assert_eq!(percentile(&s, 0.5), 3.0);
        assert_eq!(percentile(&s, 1.0), 5.0);
        assert_eq!(percentile(&[], 0.5), 0.0);
    }

    #[test]
    fn rss_helpers_return_plausible_values() {
        assert!(peak_rss_bytes() > 0);
        if let Some(rss) = current_rss_bytes() {
            assert!(rss > 0);
        }
    }

    /// End-to-end run against the seed pack when it has been built (CI builds it; skipped
    /// otherwise so the test never depends on generated data being present).
    #[test]
    fn report_runs_against_seed_pack_when_present() {
        let pack = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("data/build/packs/seed/lexicon.bin");
        if !pack.exists() {
            eprintln!("skipping: {:?} not built", pack);
            return;
        }
        let cli = Cli {
            pack,
            iterations: 5,
            loads: 2,
            probe_words: vec!["ez".into(), "li".into()],
            json: false,
        };
        let report = run(&cli).unwrap();
        assert!(report.load.entry_count > 0);
        assert!(report.load.engine_heap_bytes > 0);
        assert!(report.load.load_allocation_calls > 0);
        assert!(report.load.heap_peak_during_load_bytes >= report.load.engine_heap_bytes);
        assert!(report.peak_rss_after_reloads_bytes >= report.load.peak_rss_after_load_bytes);
        assert!(report.load.attribution_coverage > 0.0);
        assert!(report.load.attribution.total.bytes > 0);
        assert!(report.latencies.iter().any(|l| l.operation == "known_hit"));
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("attribution"));
    }
}
