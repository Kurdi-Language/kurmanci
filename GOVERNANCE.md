# Monorepo Governance & Decision-Making Policy

This document defines the decision-making structure, pull request review policies, and roadmap governance for the Kurmancî Language Platform.

---

## 1. Operational Governance

The project is maintained under single-maintainer leadership with open public community participation:

- **Maintainer**: Kurmancî Language Platform Core Maintainers (`@Kurdi-Language`).
- **Merge Authority**: Code and dataset changes to `main` require maintainer approval, clean automated CI checks, and a reviewed Pull Request.
- **Decision Records**: Architectural decisions and schema updates are recorded in `docs/` and tracked in `CHANGELOG.md` and PR discussions.

---

## 2. Planned Governance & Review Structure

As the contributor community expands, the project intends to establish a two-tier governance model:

### Technical Steering

- Responsible for Rust core engine performance, binary pack container format stability, API contracts, CLI features, and CI pipelines.

### Linguistic Review Group *(Planned)*

- Responsible for establishing documented Kurmancî review standards, reviewing proposed linguistic data, resolving supported disagreements, and auditing language-quality evidence.

Repository maintainers approve process compliance. Linguistic correctness is established by documented review evidence, not repository permissions.

## 3. Authoritative Benchmark Promotion

- Only a verified real human reviewer or documented human review body may provide the linguistic judgment required for promotion.
- Stable pseudonymous reviewer IDs are permitted when promotion authority can verify the accountable human reviewer.
- Institutional and committee IDs require traceable evidence from real human reviewers.
- AI systems, automated tools, corpus counts, source-dataset membership, and engine output cannot serve as linguistic reviewers.
- Promotion from `draft` to `human-reviewed` is metadata-only under `benchmark-case-v1`. The task, category, input, context, expectation, canonical case ID, and source provenance remain unchanged.
- If review changes a semantic field, the draft is revised and validated first. Promotion occurs only in a later focused change.
- AI-assisted and mechanical origins remain permanently recorded after human review.
- Disputed or insufficiently supported records remain drafts.
- Existing authoritative reviewed records cannot be silently changed, removed, or downgraded through the ordinary promotion workflow.
- The complete operational policy and reviewer checklist are defined in [`docs/benchmark-review.md`](docs/benchmark-review.md).

## 4. Pull Request & Contribution Policy

- **No Direct Commits to `main`**: All modifications must enter via a feature branch (`dev/...` or `feature/...`) and a Pull Request into `main`.
- **Squash-Merge Workflow**: Approved PRs are merged into `main` using **Squash and merge** to preserve a clean, bisect-friendly commit history.
- **Linguistic Dispute Escalation**: If spelling or lemma variants are disputed during review:
  1. Record the competing analyses and relevant published or human evidence.
  2. Do not treat corpus frequency, an internet search, AI output, or a source entry as sufficient evidence by itself.
  3. Leave the record in draft status until documented human linguistic review resolves the disagreement.
