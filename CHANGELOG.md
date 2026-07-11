# Changelog

All notable changes to this project are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0](https://github.com/SergioKingOne/mmdb-writer/releases/tag/v0.1.0) - 2026-07-11

### Fixed

- satisfy newer toolchains and unblock CI

### Other

- pin release-plz action to v0.5
- Initial implementation of mmdb-writer

### Added

- Initial release: write MaxMind DB (`.mmdb`) files from `(network, value)` pairs.
- `Writer` with `insert` (serde), `insert_value`, `insert_with`, `insert_merged`, and
  `insert_range`.
- Public `Value` data model covering every MMDB data type.
- IPv4 and IPv6 databases, IPv4 aliasing, and reserved-network handling.
- Automatic record sizing and pointer-based data deduplication.
- Optional `serde` (default) and `load` features.

[Unreleased]: https://github.com/SergioKingOne/mmdb-writer/commits/main
