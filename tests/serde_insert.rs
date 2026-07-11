//! Tests for the serde-backed `Writer::insert`. Only built with the `serde` feature.
#![cfg(feature = "serde")]

mod common;

use std::collections::BTreeMap;

use ipnet::IpNet;
use mmdb_writer::{Value, Writer};
use serde::{Deserialize, Serialize};

use common::lookup;

fn net(s: &str) -> IpNet {
    s.parse().expect("valid CIDR")
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct City {
    names: BTreeMap<String, String>,
    geoname_id: u32,
    is_eu: bool,
    accuracy: f64,
}

#[test]
fn struct_round_trips_through_serde() {
    let city = City {
        names: [("en".to_string(), "Example".to_string())].into(),
        geoname_id: 123,
        is_eu: false,
        accuracy: 12.5,
    };
    let mut w = Writer::new("Serde-Test");
    w.insert(net("81.2.69.0/24"), &city).unwrap();
    let bytes = w.to_bytes().unwrap();

    assert_eq!(lookup::<City>(&bytes, "81.2.69.142"), Some(city));
}

#[derive(Serialize)]
struct WithOptions {
    present: u32,
    absent: Option<u32>,
}

#[test]
fn none_fields_are_dropped() {
    let mut w = Writer::new("Serde-Test");
    w.insert(
        net("192.0.2.0/24"),
        &WithOptions {
            present: 1,
            absent: None,
        },
    )
    .unwrap();
    let bytes = w.to_bytes().unwrap();

    // The decoded map has only the `present` key.
    let decoded: BTreeMap<String, u32> = lookup(&bytes, "192.0.2.1").unwrap();
    assert_eq!(decoded.len(), 1);
    assert_eq!(decoded.get("present"), Some(&1));
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
enum Kind {
    Residential,
    Hosting,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct Tagged {
    kind: Kind,
}

#[test]
fn unit_enum_variants_serialize_as_strings() {
    let mut w = Writer::new("Serde-Test");
    w.insert(
        net("198.51.100.0/24"),
        &Tagged {
            kind: Kind::Hosting,
        },
    )
    .unwrap();
    let bytes = w.to_bytes().unwrap();

    assert_eq!(
        lookup::<Tagged>(&bytes, "198.51.100.5"),
        Some(Tagged {
            kind: Kind::Hosting
        })
    );
}

#[test]
fn i64_is_rejected() {
    #[derive(Serialize)]
    struct HasI64 {
        big: i64,
    }
    let mut w = Writer::new("Serde-Test");
    let err = w
        .insert(net("192.0.2.0/24"), &HasI64 { big: 5 })
        .unwrap_err();
    assert!(matches!(err, mmdb_writer::Error::UnsupportedValue("i64")));
}

#[test]
fn value_serializes_transparently() {
    // A `Value` inserted via the serde path must encode as its underlying data, identical to
    // inserting it via `insert_value`.
    let value = Value::map([("k", Value::from(7_u32))]);
    let mut a = Writer::builder("V")
        .build_epoch(std::time::SystemTime::UNIX_EPOCH)
        .build();
    a.insert(net("10.0.0.0/24"), &value).unwrap();
    let mut b = Writer::builder("V")
        .build_epoch(std::time::SystemTime::UNIX_EPOCH)
        .build();
    b.insert_value(net("10.0.0.0/24"), value).unwrap();
    assert_eq!(a.to_bytes().unwrap(), b.to_bytes().unwrap());
}
