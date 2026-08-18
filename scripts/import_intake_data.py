#!/usr/bin/env python3
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
    elif cat in ('transposition', 'insertion', 'deletion', 'substitution', 'correct-spelling'):
        return cat
    return cat

def map_pos(pos_str):
    pos = pos_str.strip().lower()
    if 'verb' in pos:
        return 'verb'
    elif 'noun' in pos:
        return 'noun'
    elif 'adj' in pos:
        return 'adjective'
    elif 'adv' in pos:
        return 'adverb'
    elif 'prep' in pos:
        return 'preposition'
    elif 'conj' in pos:
        return 'conjunction'
    elif 'pron' in pos:
        return 'pronoun'
    elif 'part' in pos:
        return 'particle'
    elif 'interj' in pos:
        return 'interjection'
    elif 'det' in pos:
        return 'determiner'
    return 'noun'

def process_vocabulary():
    vocab_path = 'data/benchmarks/intake/vocabulary-intake.tsv'
    lexicon_path = 'data/reviewed/lexicon.jsonl'

    existing_words = set()
    existing_entries = []
    if os.path.exists(lexicon_path):
        with open(lexicon_path, 'r', encoding='utf-8') as f:
            for line in f:
                if line.strip():
                    entry = json.loads(line)
                    existing_entries.append(entry)
                    existing_words.add(entry['word'])

    print(f"Existing lexicon words before intake: {len(existing_words)}")

    added_count = 0
    default_freq = 50000

    with open(vocab_path, 'r', encoding='utf-8') as f:
        reader = csv.DictReader(f, delimiter='\t')
        for idx, row in enumerate(reader, start=1):
            w = row['surface_form'].strip()
            if not w or w in existing_words:
                continue

            lemma = row['lemma'].strip() if row['lemma'].strip() else w
            pos = map_pos(row['pos'])

            freq = max(1000, default_freq - (idx * 90))

            entry = {
                "word": w,
                "lemma": lemma,
                "normalized": w.lower(),
                "part_of_speech": pos,
                "frequency": freq,
                "status": "verified",
                "variants": [],
                "sources": ["manual-seed"],
                "regions": ["general"]
            }

            existing_entries.append(entry)
            existing_words.add(w)
            added_count += 1

    with open(lexicon_path, 'w', encoding='utf-8') as f:
        for entry in existing_entries:
            f.write(json.dumps(entry, ensure_ascii=False) + '\n')

    print(f"✅ Merged {added_count} new entries into {lexicon_path}. Total lexicon size: {len(existing_entries)}")

def process_benchmarks():
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

    print(f"Existing case_ids: {len(seen_ids)}, existing (task, input) pairs: {len(seen_inputs)}")

    grouped_cases = {}

    with open(bench_path, 'r', encoding='utf-8') as f:
        reader = csv.DictReader(f, delimiter='\t')
        for row in reader:
            raw_task = row['task'].strip()
            if raw_task == 'predict-next':
                continue

            task = raw_task
            raw_cat = row['category'].strip()
            category = map_category(raw_cat)
            inp = row['input'].strip()

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
            elif task in ('correct-word', 'complete-prefix'):
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
        elif task in ('correct-word', 'complete-prefix'):
            if g['expected_candidates']:
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

    print(f"✅ Written {len(cases)} deduplicated benchmark cases to {draft_path}")

if __name__ == '__main__':
    process_vocabulary()
    process_benchmarks()
