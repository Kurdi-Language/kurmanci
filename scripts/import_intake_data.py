#!/usr/bin/env python3
"""
Intake and Review Queue Tooling for Kurmancî Platform.

This script:
1. Generates/updates the canonical machine-readable vocabulary review queue (data/benchmarks/intake/vocabulary-intake.tsv)
   from provenanced sources (data/imported/kurdish-hunspell-kmr/lexicon.jsonl) and real corpus frequencies (data/build/frequencies.jsonl).
   - DOES NOT mutate data/reviewed/lexicon.jsonl or promote candidates automatically.
   - Uses real corpus token/document counts (or empty string if unavailable; no fabricated frequencies).
   - Maps POS accurately and leaves ambiguous POS as 'unknown' (never defaults to 'noun').
   - Sets review status to 'pending'.

2. Generates reproducible draft benchmark cases (evaluation/spelling/draft-cases.jsonl)
   from committed intake specifications (evaluation/spelling/intake/benchmark-intake-300.tsv).
   - All source_document_id references point to actual committed files.
   - expected_candidates is consistently a JSON array.
   - Cases remain marked as review_status: 'draft'.
"""

import csv
import json
import hashlib
import os

BENCHMARK_CASE_DOMAIN_TAG = "kurmanci-spelling-case-v1"

def u64_be(val):
    return val.to_bytes(8, byteorder='big')

def encode_str(s):
    b = s.encode('utf-8')
    return u64_be(len(b)) + b

def encode_opt_str(s):
    if s is None:
        return b'\x00'
    return b'\x01' + encode_str(s)

def encode_str_array(arr):
    sorted_arr = sorted(list(set(arr)))
    res = u64_be(len(sorted_arr))
    for item in sorted_arr:
        res += encode_str(item)
    return res

def encode_opt_bool(opt):
    if opt is None:
        return b'\x00'
    elif opt is True:
        return b'\x01\x01'
    else:
        return b'\x01\x00'

def encode_opt_usize(opt):
    if opt is None:
        return b'\x00'
    else:
        return b'\x01' + u64_be(opt)

def encode_expectation(exp):
    accepted = exp.get("accepted")
    preserve_exact = exp.get("preserve_exact")
    expected_candidates = sorted(list(set(exp.get("expected_candidates", []))))
    forbidden_candidates = sorted(list(set(exp.get("forbidden_candidates", []))))
    allow_no_candidate = exp.get("allow_no_candidate")
    required_top_k = exp.get("required_top_k")

    res = bytearray()
    res.extend(encode_opt_bool(accepted))
    res.extend(encode_opt_bool(preserve_exact))
    res.extend(encode_str_array(expected_candidates))
    res.extend(encode_str_array(forbidden_candidates))
    res.extend(encode_opt_bool(allow_no_candidate))
    res.extend(encode_opt_usize(required_top_k))
    return bytes(res)

def compute_case_id(task, category, input_str, context, expectation):
    res = bytearray()
    res.extend(encode_str(BENCHMARK_CASE_DOMAIN_TAG))
    res.extend(encode_str(task))
    res.extend(encode_str(category))
    res.extend(encode_str(input_str))

    if context:
        res.extend(encode_str_array(context))
    else:
        res.extend(encode_str_array([]))

    res.extend(encode_expectation(expectation))
    return hashlib.sha256(bytes(res)).hexdigest()

def map_category(cat):
    if cat.startswith('missing-diacritics') or cat == 'wrong-diacritic':
        return 'missing-diacritics'
    elif cat in ('multiple-missing-diacritics', 'combined-errors'):
        return 'multi-edit'
    elif cat == 'repeated-letter':
        return 'insertion'
    elif cat == 'exact-preservation':
        return 'exact-preservation'
    elif cat == 'false-acceptance':
        return 'false-acceptance'
    elif cat == 'prefix-completion':
        return 'prefix-completion'
    elif cat in ('transposition', 'insertion', 'deletion', 'substitution', 'correct-spelling', 'trigram-context'):
        return cat
    return cat

def map_pos(raw_pos):
    pos = raw_pos.strip().lower()
    if pos in ('noun', 'noun_masc', 'noun_fem'):
        return 'noun'
    elif pos in ('adj', 'adjective'):
        return 'adjective'
    elif pos in ('verb', 'verb_transitive', 'verb_intransitive'):
        return 'verb'
    elif pos in ('adv', 'adverb'):
        return 'adverb'
    elif pos in ('adp', 'prep', 'preposition'):
        return 'preposition'
    elif pos in ('sconj', 'cconj', 'conj', 'conjunction'):
        return 'conjunction'
    elif pos in ('pronoun', 'pron'):
        return 'pronoun'
    elif pos in ('part', 'particle'):
        return 'particle'
    elif pos in ('interj', 'interjection'):
        return 'interjection'
    elif pos in ('det', 'determiner'):
        return 'determiner'
    else:
        return 'unknown'

def process_vocabulary():
    """
    Extracts candidate entries from KurdHunspell and real corpus frequencies,
    deduplicates and ranks them, and builds a 1,000+ candidate review queue TSV.
    DOES NOT modify data/reviewed/lexicon.jsonl.
    """
    imported_hunspell_path = 'data/imported/kurdish-hunspell-kmr/lexicon.jsonl'
    freq_path = 'data/build/frequencies.jsonl'
    intake_tsv_path = 'data/benchmarks/intake/vocabulary-intake.tsv'

    if not os.path.exists(imported_hunspell_path):
        print(f"⚠️ Warning: {imported_hunspell_path} missing. Skipping vocabulary queue build.")
        return

    with open(imported_hunspell_path, 'r', encoding='utf-8') as f:
        entries = [json.loads(line) for line in f if line.strip()]

    freq_map = {}
    if os.path.exists(freq_path):
        with open(freq_path, 'r', encoding='utf-8') as f:
            for line in f:
                if line.strip():
                    d = json.loads(line)
                    freq_map[d['word']] = (d.get('token_count', 0), d.get('document_count', 0))

    candidates = []
    seen = set()

    for e in entries:
        w = e.get('word', '').strip()
        raw_pos = e.get('part_of_speech', '')
        if not w or raw_pos in ('punctuation', 'symbol', 'digit'):
            continue

        # Exclude punctuation/digit artifacts
        if any(c in w for c in "#$%&()*+,-./:;<=>?@[\\]^_`{|}~\t\r\n 0123456789"):
            continue

        if w in seen:
            continue
        seen.add(w)

        pos = map_pos(raw_pos)
        tc, dc = freq_map.get(w, (0, 0))

        candidates.append({
            'word': w,
            'lemma': e.get('lemma', w),
            'pos': pos,
            'token_count': tc,
            'doc_count': dc,
            'source_ids': 'kurdish-hunspell-kmr'
        })

    # Sort by real token_count descending, doc_count descending, then word ascending
    candidates.sort(key=lambda x: (-x['token_count'], -x['doc_count'], x['word']))

    # Take top 1050 candidates for human review queue
    queue_candidates = candidates[:1050]

    fieldnames = [
        "candidate_id", "surface_form", "normalized_form", "lemma", "pos",
        "gloss_note", "region", "register", "domain", "form_type",
        "token_count", "document_count", "source_count", "source_ids",
        "example_1", "example_2", "diacritic_collision", "proper_name_risk",
        "foreign_language_risk", "priority", "decision", "reviewer_id",
        "review_date", "review_notes"
    ]

    with open(intake_tsv_path, 'w', encoding='utf-8', newline='') as f:
        writer = csv.DictWriter(f, fieldnames=fieldnames, delimiter='\t')
        writer.writeheader()

        for idx, item in enumerate(queue_candidates, start=1):
            cid = f"V-{idx:04d}"
            tc_str = str(item['token_count']) if item['token_count'] > 0 else ""
            dc_str = str(item['doc_count']) if item['doc_count'] > 0 else ""

            priority = "high" if item['token_count'] > 0 else "medium"

            writer.writerow({
                "candidate_id": cid,
                "surface_form": item['word'],
                "normalized_form": item['word'].lower(),
                "lemma": item['lemma'],
                "pos": item['pos'],
                "gloss_note": "",
                "region": "general",
                "register": "neutral",
                "domain": "everyday",
                "form_type": "lemma-or-common-form",
                "token_count": tc_str,
                "document_count": dc_str,
                "source_count": "1",
                "source_ids": item['source_ids'],
                "example_1": "",
                "example_2": "",
                "diacritic_collision": "low",
                "proper_name_risk": "low",
                "foreign_language_risk": "",
                "priority": priority,
                "decision": "pending",
                "reviewer_id": "",
                "review_date": "",
                "review_notes": ""
            })

    print(f"✅ Generated {len(queue_candidates)} candidate review slots in {intake_tsv_path}")

def process_benchmarks():
    """
    Generates reproducible draft benchmark cases from committed intake specs (benchmark-intake-300.tsv).
    Writes to evaluation/spelling/draft-cases.jsonl.
    """
    bench_path = 'evaluation/spelling/intake/benchmark-intake-300.tsv'
    draft_path = 'evaluation/spelling/draft-cases.jsonl'
    reviewed_path = 'evaluation/spelling/reviewed-cases.jsonl'
    cases_path = 'evaluation/spelling/cases.jsonl'

    seen_ids = set()
    seen_inputs = set()

    for p in [reviewed_path, cases_path]:
        if os.path.exists(p):
            with open(p, 'r', encoding='utf-8') as f:
                for line in f:
                    if line.strip():
                        r = json.loads(line)
                        if 'case_id' in r:
                            seen_ids.add(r['case_id'])
                        if 'task' in r and 'input' in r:
                            seen_inputs.add((r['task'], r['input']))

    grouped_cases = {}

    with open(bench_path, 'r', encoding='utf-8') as f:
        reader = csv.DictReader(f, delimiter='\t')
        for row in reader:
            raw_task = row['task'].strip()
            task = raw_task
            raw_cat = row['category'].strip()
            category = map_category(raw_cat)
            inp = row['input'].strip()

            if not inp:
                continue

            if (task, inp) in seen_inputs:
                continue

            key = (task, category, inp)
            if key not in grouped_cases:
                grouped_cases[key] = {
                    'accepted': None,
                    'preserve_exact': None,
                    'expected_candidates': set(),
                    'forbidden_candidates': set(),
                    'allow_no_candidate': None,
                    'required_top_k': 5
                }

            g = grouped_cases[key]

            if task == 'accept-word':
                g['accepted'] = row['accepted_boolean'].strip().lower() == 'true'
                if row['preserve_exact'].strip().lower() == 'true':
                    g['preserve_exact'] = True
            elif task in ('correct-word', 'complete-prefix', 'predict-next'):
                for k in ['expected_candidate_1', 'expected_candidate_2', 'expected_candidate_3']:
                    c = row[k].strip()
                    if c:
                        g['expected_candidates'].add(c)
                top_k = row['required_top_k'].strip()
                if top_k:
                    g['required_top_k'] = max(g['required_top_k'], int(top_k))
                if row['allow_no_candidate'].strip().lower() == 'true':
                    g['allow_no_candidate'] = True

    cases = []
    for (task, category, inp), g in grouped_cases.items():
        expectation = {}
        if task == 'accept-word':
            expectation['accepted'] = g['accepted']
            if g['preserve_exact'] is not None:
                expectation['preserve_exact'] = g['preserve_exact']
        elif task in ('correct-word', 'complete-prefix', 'predict-next'):
            if not g['expected_candidates']:
                # Skip intake slots that lack expected candidates until human review adds them
                continue
            expectation['expected_candidates'] = sorted(list(g['expected_candidates']))
            expectation['required_top_k'] = g['required_top_k']
            if g['allow_no_candidate'] is not None:
                expectation['allow_no_candidate'] = g['allow_no_candidate']

        context = None
        case_id = compute_case_id(task, category, inp, context, expectation)

        if case_id in seen_ids:
            continue
        seen_ids.add(case_id)

        record = {
            "schema_version": "benchmark-case-v1",
            "case_id": case_id,
            "task": task,
            "category": category,
            "input": inp,
            "expectation": expectation,
            "review_status": "draft",
            "reviewer_id": None,
            "review_date": None,
            "review_notes": None,
            "source": {
                "kind": "ai-assisted-draft",
                "source_id": "kurmanci-data-enrichment-starter-v1",
                "source_document_id": "evaluation/spelling/intake/benchmark-intake-300.tsv"
            }
        }
        cases.append(record)

    with open(draft_path, 'w', encoding='utf-8') as f:
        for c in cases:
            f.write(json.dumps(c, ensure_ascii=False) + '\n')

    print(f"✅ Written {len(cases)} deduplicated draft benchmark cases to {draft_path}")

if __name__ == '__main__':
    process_vocabulary()
    process_benchmarks()
