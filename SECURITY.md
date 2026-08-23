# Security Policy & Privacy Guarantees

## Privacy & Security Architecture

The Kurmancî core engine (`kurmanci-engine`) and compiler (`kurmanci-data-builder`) operate on strict offline principles:

1. **Zero Network Dependencies**: The current Rust engine performs no network requests and includes no telemetry dependencies.
2. **Local Transient Memory**: Input strings passed to query APIs (`suggest`, `contains`) are processed transiently in memory for candidate generation and immediately discarded.
3. **No Automatic Persistence**: The core engine library does not write user input or query logs to disk.

Mobile keyboard integrations, personal dictionaries, and platform-specific privacy guarantees are planned for future platform updates.

## Supported Versions

| Version | Supported |
| ------- | ------------------ |
| `0.1.x` | :white_check_mark: |
| `< 0.1` | :x:                |

## Vulnerability Reporting

If you discover a security issue or potential privacy defect in this library:

- Report security concerns privately via [GitHub Security Vulnerability Reporting](https://github.com/Kurdi-Language/kurmanci/security/advisories/new) or private maintainer email (`security@kurmanci.org`).
- Do not open public issue tracker threads for unpatched security or data leakage concerns.
- Maintainers aim to acknowledge reports within seven days and will provide updates when practicable.
