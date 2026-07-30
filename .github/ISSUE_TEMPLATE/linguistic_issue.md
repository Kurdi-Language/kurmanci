name: Linguistic Data Issue
description: Report a spelling, diacritic, lemma, or vocabulary issue in Kurmancî data
title: "[LINGUISTIC] "
labels: ["linguistic-data"]
assignees: []
body:
  - type: markdown
    attributes:
      value: |
        Report orthographic, diacritic, lemma, POS, or vocabulary issues in the Kurmancî lexicon.
  - type: input
    id: word
    attributes:
      label: Target Word or Phrase
      description: The exact Kurmancî word or phrase in question.
      placeholder: e.g. rojbaş
    validations:
      required: true
  - type: input
    id: correction
    attributes:
      label: Proposed Correction
      description: The corrected spelling, lemma, or POS tag.
    validations:
      required: true
  - type: dropdown
    id: region
    attributes:
      label: Region or Variety
      description: Sub-region or dialectal variety where applicable.
      options:
        - general (Standard Kurmancî)
        - ku-Latn-TR (Turkey)
        - ku-Latn-SY (Syria)
        - ku-Latn-IQ (Iraq)
    validations:
      required: true
  - type: input
    id: source
    attributes:
      label: Source Reference
      description: Dictionary, grammar reference, literary text, or authoritative source.
    validations:
      required: true
  - type: textarea
    id: examples
    attributes:
      label: Usage Examples & Context
      description: Provide sample sentences illustrating correct usage.
      placeholder: e.g. "Rojbaş hevalno, hûn çawan in?"
    validations:
      required: true
  - type: textarea
    id: explanation
    attributes:
      label: Detailed Explanation
      description: Further grammatical or orthographic context.
    validations:
      required: false
