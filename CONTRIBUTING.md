# Contributing to mmdb-writer

Thanks for your interest in improving `mmdb-writer`! Contributions of all kinds are welcome:
bug reports, documentation fixes, and pull requests.

## Development

```bash
cargo build --all-features
cargo test  --all-features
cargo test  --no-default-features   # the value-only API must also pass
```

Before opening a pull request, please make sure the following are clean — these are the same
checks CI runs:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features
```

## Correctness

The library is verified by round-tripping every written database through the official
[`maxminddb`](https://crates.io/crates/maxminddb) reader — see the `tests/` directory. New
behavior should come with a test that writes a database and reads it back.

For deeper checks:

```bash
cargo test --all-features -- --ignored   # large stress tests
cargo mutants                            # mutation testing (cargo install cargo-mutants)
cargo llvm-cov --all-features            # coverage (cargo install cargo-llvm-cov)
```

## Commit messages

This project uses [Conventional Commits](https://www.conventionalcommits.org/) so releases
and the changelog can be generated automatically (e.g. `feat:`, `fix:`, `docs:`).

## License

By contributing, you agree that your contributions will be dual licensed under the MIT and
Apache-2.0 licenses, without any additional terms or conditions.
