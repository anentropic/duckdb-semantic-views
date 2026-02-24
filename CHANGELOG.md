# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Initial extension scaffold using `duckdb/extension-template-rs`
- Multi-platform CI build matrix (Linux x86_64/arm64, macOS x86_64/arm64, Windows x86_64)
- Scheduled DuckDB version monitor with automated PR creation
- Code quality gates: `rustfmt`, `clippy` (pedantic), `cargo-deny`, 80% coverage
- Developer task runner (`just`) with `just setup` one-command dev environment
- Pre-commit hooks via `cargo-husky` (rustfmt + clippy)

[Unreleased]: https://github.com/paul-rl/duckdb-semantic-views/compare/HEAD...HEAD
