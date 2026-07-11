//! Loading an existing database, extending it, and re-serializing. Only built with the
//! `load` feature.
#![cfg(feature = "load")]

mod common;

use std::time::{Duration, SystemTime};

use ipnet::IpNet;
use mmdb_writer::{RecordSize, Value, Writer};
use serde::Deserialize;

use common::{lookup, reader};

fn net(s: &str) -> IpNet {
    s.parse().expect("valid CIDR")
}

#[derive(Debug, PartialEq, Deserialize)]
struct Rec {
    name: String,
    n: u32,
}

#[test]
fn load_extend_and_reserialize() {
    // Build an original database.
    let mut w = Writer::new("Loaded-DB");
    w.insert_value(
        net("1.2.3.0/24"),
        Value::map([("name", Value::from("original")), ("n", Value::from(1_u32))]),
    )
    .unwrap();
    let original = w.to_bytes().unwrap();

    // Load, extend, and write again.
    let mut loaded = Writer::load(&original).unwrap();
    loaded
        .insert_value(
            net("4.5.6.0/24"),
            Value::map([("name", Value::from("added")), ("n", Value::from(2_u32))]),
        )
        .unwrap();
    let extended = loaded.to_bytes().unwrap();

    // Both the original and the added network resolve.
    assert_eq!(
        lookup::<Rec>(&extended, "1.2.3.4"),
        Some(Rec {
            name: "original".into(),
            n: 1
        })
    );
    assert_eq!(
        lookup::<Rec>(&extended, "4.5.6.7"),
        Some(Rec {
            name: "added".into(),
            n: 2
        })
    );
}

#[test]
fn load_preserves_metadata() {
    let epoch = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    let mut w = Writer::builder("Metadata-DB")
        .languages(["en", "de"])
        .record_size(RecordSize::Bits32)
        .build_epoch(epoch)
        .build();
    w.insert_value(net("10.0.0.0/24"), Value::from(1_u32))
        .unwrap();
    let original = w.to_bytes().unwrap();

    let loaded = Writer::load(&original).unwrap();
    let bytes = loaded.to_bytes().unwrap();

    let reader = reader(&bytes);
    let meta = reader.metadata();
    assert_eq!(meta.database_type, "Metadata-DB");
    assert_eq!(meta.languages, vec!["en".to_string(), "de".to_string()]);
    assert_eq!(meta.record_size, 32);
    assert_eq!(meta.build_epoch, 1_700_000_000);
}

#[test]
fn ipv6_data_survives_a_load() {
    let mut w = Writer::new("V6-DB");
    w.insert_value(net("2001:db8::/32"), Value::from(7_u32))
        .unwrap();
    let original = w.to_bytes().unwrap();

    let loaded = Writer::load(&original).unwrap();
    let bytes = loaded.to_bytes().unwrap();
    assert_eq!(lookup::<u32>(&bytes, "2001:db8::1"), Some(7));
}
