# Benchmark Review Governance

This document governs how Kurmancî benchmark records become authoritative. It does not establish any linguistic decision or promote any benchmark case.

## Lifecycle

The only serialized review states in `benchmark-case-v1` are `draft` and `human-reviewed`.

1. **Draft creation or revision** — A proposed case is stored in `evaluation/spelling/draft-cases.jsonl`. Drafts may be created, revised, or removed and are never authoritative.
2. **Human linguistic review** — A real reviewer or documented human review body checks the complete case and its evidence.
3. **Promotion** — An approved draft is moved to `evaluation/spelling/reviewed-cases.jsonl` with valid review metadata. Promotion is metadata-only.
4. **Evaluation** — Only records in the authoritative reviewed file with `review_status = "human-reviewed"` may contribute authoritative evaluation results.

No automated process, repository permission, engine output, or source-dataset membership may bypass human review.

## Reviewer identities and responsibilities

Reviewer IDs are stable pseudonymous identifiers. They contain 3 to 64 ASCII bytes, match `^[a-z0-9]+(?:-[a-z0-9]+)*$`, and contain at least one lowercase ASCII letter. Numeric-only segmented IDs are rejected. Preferred namespaces include `reviewer-`, `institution-`, and `committee-`, followed by stable identifying segments.

The segments `ai`, `auto`, `automatic`, `bot`, `system`, `assistant`, and `chatgpt` are reserved and rejected anywhere in a reviewer ID. Purely numeric IDs are also rejected.

Valid syntax does not prove human identity. Promotion authority must verify that the identifier belongs to a real human reviewer or a documented human review body. Institutional and committee IDs require traceable human linguistic evidence.

A reviewer must:

- understand the task and category being reviewed;
- examine the input, context, expectation, alternatives, and source provenance;
- verify normalization and capitalization;
- record the real date on which the linguistic judgment was completed;
- add notes when the decision is not self-evident;
- disclose uncertainty or disagreement;
- never erase AI-assisted or mechanical origin.

The `review_date` is not the draft creation date, evidence publication date, or pull-request merge date. It uses the proleptic Gregorian calendar in exact `YYYY-MM-DD` form from `0001-01-01` through `9999-12-31`.

## Promotion semantics

For `benchmark-case-v1`, draft-to-reviewed promotion is metadata-only.

These fields must remain unchanged:

- `schema_version`;
- `case_id`;
- `task`;
- `category`;
- `input`;
- `context`;
- `expectation`;
- `source` and every source-provenance field.

Only these fields may change:

- `review_status`;
- `reviewer_id`;
- `review_date`;
- `review_notes`.

A valid promotion removes the semantic case from the candidate draft file and adds it to the candidate reviewed file. The transition validator compares explicit base and candidate snapshots; it does not infer promotion from a final record or invoke Git.

If review changes an expected answer, task, category, input, context, or source record, revise the record while it is still a draft and recompute its canonical `case_id`. Validate that revised draft in a focused change. It may be promoted only later, after the revised draft has been reviewed.

When multiple spellings are valid, every accepted answer must be listed explicitly before promotion. A preferred answer must not silently exclude other accepted forms.

## Evidence standards

Acceptable supporting evidence may include:

- documented native-speaker review;
- a relevant published dictionary;
- a relevant grammar reference;
- agreement among qualified human reviewers.

The following are not sufficient by themselves:

- AI output;
- a Hunspell entry;
- corpus frequency;
- an internet search result;
- current engine behavior;
- agreement with another generated dataset.

Evidence must be relevant to the exact judgment. A source entry proves provenance, not correctness. Human approval does not erase the original source or AI-assisted origin.

## Disagreement and uncertainty

Disputed or insufficiently supported cases remain drafts. Reviewers record the competing analyses and supporting evidence instead of forcing consensus.

Where several qualified reviewers disagree, promotion requires either documented agreement or an explicit decision by the project’s recognized linguistic review authority. Repository maintainers may verify that this process was followed, but repository permissions do not establish linguistic correctness.

## Authoritative history and corrections

Existing reviewed records are immutable in the ordinary promotion workflow. They may not be silently modified, removed, or downgraded to drafts.

A future correction or removal workflow must:

- document the reason and supporting human evidence;
- preserve the previous decision in repository history;
- distinguish correction from ordinary promotion;
- define any required schema or benchmark-data version change.

Until that workflow exists, the transition validator rejects reviewed-record modification, removal, and downgrade.

## Independent versions

These versions evolve independently:

- **Benchmark schema version** — the serialized case contract, currently `benchmark-case-v1`.
- **Benchmark data version** — the reviewed dataset release; it is not declared until authoritative reviewed data exists.
- **Engine version** — implementation and public behavior.
- **Comparison-policy version** — metric and comparison semantics, currently `three-pack-comparison-v1`.

A change in one version does not automatically change the others.

## Reviewer checklist

Before promotion, confirm:

- spelling and normalization are correct;
- capitalization is correct;
- task and category are correct;
- input and context are correct;
- expectation and expected engine behavior are correct;
- all accepted alternatives are explicit;
- source provenance is complete and unchanged;
- AI-assisted or mechanical origin is preserved;
- reviewer ID and review date are valid and genuine;
- review notes explain non-obvious decisions;
- disagreement or uncertainty has been resolved with documented evidence.

## Validation commands

Validate the current repository case set:

```bash
cargo run -p kurmanci-data-builder -- validate-eval-cases
```

Validate explicit base and candidate snapshots:

```bash
cargo run -p kurmanci-data-builder -- validate-eval-transition \
  --base-draft <path> \
  --base-reviewed <path> \
  --candidate-draft <path> \
  --candidate-reviewed <path>
```

The transition validator permits ordinary draft creation, revision, and removal. It protects all existing authoritative reviewed records and accepts a new reviewed case only as a metadata-only promotion of a matching base draft.
