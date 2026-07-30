name: Bug Report
description: Create a report to help us improve the Kurmancî Language Platform
title: "[BUG] "
labels: ["bug"]
assignees: []
body:
  - type: markdown
    attributes:
      value: |
        Thank you for reporting a bug! Please fill out the sections below.
  - type: textarea
    id: description
    attributes:
      label: Bug Description
      description: A clear and concise description of what the bug is.
      placeholder: Explain what happened...
    validations:
      required: true
  - type: textarea
    id: reproduction
    attributes:
      label: Steps to Reproduce
      description: Exact commands or code to reproduce the behavior.
      placeholder: |
        1. Run command '...'
        2. Call function '...'
        3. See error
    validations:
      required: true
  - type: textarea
    id: expected
    attributes:
      label: Expected Behavior
      description: A clear description of what you expected to happen.
    validations:
      required: true
  - type: textarea
    id: environment
    attributes:
      label: Environment Info
      description: OS version, Rust toolchain version (`rustc --version`), crate version.
      placeholder: macOS / Linux, Rust 1.85.0, kurmanci-engine v0.1.0 (or commit SHA)
    validations:
      required: false
