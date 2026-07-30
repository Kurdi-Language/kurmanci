# Monorepo Governance & Decision-Making Policy

This document defines the decision-making structure, pull request review policies, and roadmap governance for the Kurmancî Language Platform.

---

## 1. Operational Governance (Current Phase)

During the initial platform setup phase, the project is maintained under single-maintainer leadership with open public community participation:

- **Maintainer**: Kurmancî Language Platform Core Maintainers (`@Kurdi-Language`).
- **Merge Authority**: Code and dataset changes to `main` require maintainer approval, clean automated CI checks, and a reviewed Pull Request.
- **Decision Records**: Architectural decisions and schema updates are recorded in `docs/` and tracked in `CHANGELOG.md` and PR discussions.

---

## 2. Planned Governance & Review Structure

As the contributor community expands, the project intends to establish a two-tier governance model:

### Technical Steering
- Responsible for Rust core engine performance, binary pack container format stability, API contracts, CLI features, and CI pipelines.

### Linguistic Review Group *(Planned)*
- Responsible for establishing standardized Kurmancî orthographic guidelines, reviewing external dictionary imports (e.g. Wiktionary, Hunspell), resolving regional variation disputes (`ku-Latn`), and auditing word frequency ranks.

---

## 3. Pull Request & Contribution Policy

- **No Direct Commits to `main`**: All modifications must enter via a feature branch (`dev/...` or `feature/...`) and a Pull Request into `main`.
- **Squash-Merge Workflow**: Approved PRs are merged into `main` using **Squash and merge** to preserve a clean, bisect-friendly commit history.
- **Linguistic Dispute Escalation**: If spelling or lemma variants are disputed during review:
  1. Primary authority is deferred to established Kurmancî dictionary references and corpus evidence.
  2. If regional variation exists, entries are tagged with appropriate region tags (`regions = ["ku-Latn-TR"]`) rather than rejecting non-standard regional entries.
