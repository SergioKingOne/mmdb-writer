# Changelog

All notable changes to this project are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-07-12

### Added

- Initial release: write MaxMind DB (`.mmdb`) files from `(network, value)` pairs.
- `Writer` with `insert` (serde), `insert_value`, `insert_with`, `insert_merged`,
  `insert_range`, and `get`.
- Public `Value` data model covering every MMDB data type.
- IPv4 and IPv6 databases, IPv4 aliasing (IPv4-mapped, 6to4, Teredo), and
  reserved-network handling with Go-writer-compatible carve-out semantics.
- Automatic record sizing (24/28/32-bit) and pointer-based data deduplication.
- Optional `serde` (default) and `load` features.
- Output verified against the official Rust, Python, and Go MaxMind readers, with
  byte-exact spec-conformance tests and a mutation-tested suite.

[Unreleased]: https://github.com/SergioKingOne/mmdb-writer/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/SergioKingOne/mmdb-writer/releases/tag/v0.1.0
