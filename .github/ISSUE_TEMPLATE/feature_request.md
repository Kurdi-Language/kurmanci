name: Feature Request
description: Suggest an idea or enhancement for the Kurmancî Language Platform
title: "[FEATURE] "
labels: ["enhancement"]
assignees: []
body:
  - type: markdown
    attributes:
      value: |
        Thank you for proposing a feature or enhancement!
  - type: textarea
    id: problem
    attributes:
      label: Problem Statement / Motivation
      description: Is your feature request related to a problem or limitation?
      placeholder: Describe the problem or goal...
    validations:
      required: true
  - type: textarea
    id: proposal
    attributes:
      label: Proposed Solution
      description: Describe the solution or capability you would like added.
    validations:
      required: true
  - type: textarea
    id: alternatives
    attributes:
      label: Alternatives Considered
      description: Describe any alternative solutions or features you've considered.
    validations:
      required: false
