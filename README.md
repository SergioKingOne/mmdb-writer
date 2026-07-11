# mmdb-writer

[![crates.io](https://img.shields.io/crates/v/mmdb-writer.svg)](https://crates.io/crates/mmdb-writer)
[![docs.rs](https://img.shields.io/docsrs/mmdb-writer)](https://docs.rs/mmdb-writer)
[![CI](https://github.com/SergioKingOne/mmdb-writer/actions/workflows/ci.yml/badge.svg)](https://github.com/SergioKingOne/mmdb-writer/actions/workflows/ci.yml)
[![MSRV](https://img.shields.io/badge/MSRV-1.85-blue)](https://blog.rust-lang.org/)
[![License](https://img.shields.io/crates/l/mmdb-writer.svg)](#license)

Write [MaxMind DB](https://maxmind.github.io/MaxMind-DB/) (`.mmdb`) files in pure, safe Rust.

Build an IP-address-to-data lookup database — the same on-disk format used by GeoIP2 /
GeoLite2 — that any MaxMind-compatible reader can consume: the
[`maxminddb`](https://crates.io/crates/maxminddb) crate, `mmdbinspect`, the official Go and
Python readers, and so on.

`#![forbid(unsafe_code)]`. The only required dependencies are
[`ipnet`](https://crates.io/crates/ipnet), [`thiserror`](https://crates.io/crates/thiserror),
and [`bon`](https://crates.io/crates/bon).

## Quickstart

```toml
[dependencies]
mmdb-writer = "0.1"
```

```rust
use ipnet::IpNet;
use mmdb_writer::Writer;
use serde::Serialize;

#[derive(Serialize)]
struct City {
    names: std::collections::BTreeMap<String, String>,
    geoname_id: u32,
}

# fn main() -> Result<(), Box<dyn std::error::Error>> {
let mut writer = Writer::new("My-City-DB");

let record = City {
    names: [("en".to_string(), "Example City".to_string())].into(),
    geoname_id: 123,
};
writer.insert("192.0.2.0/24".parse::<IpNet>()?, &record)?;

let bytes: Vec<u8> = writer.to_bytes()?;
std::fs::write("city.mmdb", bytes)?;
# std::fs::remove_file("city.mmdb").ok();
# Ok(())
# }
```

Read it back with the `maxminddb` crate to confirm:

```rust,ignore
let reader = maxminddb::Reader::open_readfile("city.mmdb")?;
let city: City = reader.lookup("192.0.2.42".parse()?)?.unwrap();
assert_eq!(city.geoname_id, 123);
```

## Features

| Feature  | Default | Description                                                                 |
| -------- | :-----: | --------------------------------------------------------------------------- |
| `serde`  |   ✅    | `Writer::insert` accepts any `serde::Serialize`; `Value` is `Serialize`/`Deserialize`. |
| `load`   |   —     | `Writer::load` reads an existing `.mmdb` (via `maxminddb`) to extend it. Implies `serde`. |

Disable defaults to build the value-only API with no `serde` dependency:

```toml
mmdb-writer = { version = "0.1", default-features = false }
```

```rust
use ipnet::IpNet;
use mmdb_writer::{Value, Writer};

# fn main() -> Result<(), Box<dyn std::error::Error>> {
let mut writer = Writer::new("My-ASN-DB");
let value = Value::map([("autonomous_system_number", Value::from(64_512_u32))]);
writer.insert_value("192.0.2.0/24".parse::<IpNet>()?, value)?;
let _bytes = writer.to_bytes()?;
# Ok(())
# }
```

## Capabilities

- **IPv4 and IPv6** databases (`ip_version` 4 or 6), with IPv4 automatically reachable from
  the IPv4-mapped, 6to4, and Teredo ranges via tree aliasing.
- **All MMDB data types**: map, array, string, bytes, double, float, boolean, and unsigned
  integers up to 128-bit plus signed 32-bit.
- **Insert strategies**: last-write-wins replace (default), custom read-modify-write via
  `insert_with`, and top-level / deep map merges.
- **`insert_range`** for arbitrary start–end IP ranges (decomposed into CIDR blocks).
- **Automatic record sizing** (24 / 28 / 32-bit) and pointer-based data deduplication for
  compact output, with a deterministic byte layout for reproducible builds.

## How this compares

[`maxminddb-writer`](https://crates.io/crates/maxminddb-writer) is the other Rust writer; it
is minimal and low-level. `mmdb-writer` aims for parity with the official Go
[`mmdbwriter`](https://github.com/maxmind/mmdbwriter): a typed `Value` model, insert
strategies, IPv4/IPv6 support, reserved-network handling, and thorough documentation.

## Minimum supported Rust version

The MSRV is **1.85** (required by edition 2024). Raising it is a minor-version change.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for
inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed
as above, without any additional terms or conditions.
