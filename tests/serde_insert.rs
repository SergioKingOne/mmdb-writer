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
fn u8_and_some_fields_round_trip() {
    // u8 widens to the MMDB uint16 type; Some(x) must serialize as x, not be dropped.
    #[derive(Serialize)]
    struct Narrow {
        small: u8,
        opt: Option<u32>,
    }
    #[derive(Deserialize, Debug, PartialEq)]
    struct NarrowOut {
        small: u16,
        opt: u32,
    }
    let mut w = Writer::new("Serde-Test");
    w.insert(
        net("192.0.2.0/24"),
        &Narrow {
            small: 255,
            opt: Some(7),
        },
    )
    .unwrap();
    let bytes = w.to_bytes().unwrap();
    assert_eq!(
        lookup::<NarrowOut>(&bytes, "192.0.2.1"),
        Some(NarrowOut { small: 255, opt: 7 })
    );
}

/// Exercises every serde shape the serializer supports in one record: narrow signed ints,
/// char, newtype structs, tuples, tuple structs, and all three non-unit enum variant forms.
///
/// Verification decodes into `serde_json::Value` (the reader's derive-based deserializer
/// cannot reconstruct Rust-side newtypes/enums, but the *written bytes* must follow serde's
/// canonical data model: newtypes transparent, tuples as arrays, enums externally tagged).
#[test]
fn full_serde_type_surface_round_trips() {
    #[derive(Serialize)]
    struct Meters(u32); // newtype struct

    #[derive(Serialize)]
    struct Pair(u16, String); // tuple struct

    #[derive(Serialize)]
    enum Shape {
        Point(u32),              // newtype variant
        Line(u32, u32),          // tuple variant
        Rect { w: u32, h: u32 }, // struct variant
    }

    #[derive(Serialize)]
    struct Surface {
        tiny: i8,
        small: i16,
        letter: char,
        depth: Meters,
        pair: Pair,
        tuple: (u32, String),
        newtype_variant: Shape,
        tuple_variant: Shape,
        struct_variant: Shape,
    }

    let record = Surface {
        tiny: -5,
        small: -300,
        letter: 'x',
        depth: Meters(42),
        pair: Pair(7, "seven".into()),
        tuple: (1, "one".into()),
        newtype_variant: Shape::Point(9),
        tuple_variant: Shape::Line(3, 4),
        struct_variant: Shape::Rect { w: 10, h: 20 },
    };

    let mut w = Writer::new("Serde-Surface");
    w.insert(net("192.0.2.0/24"), &record).unwrap();
    let bytes = w.to_bytes().unwrap();

    let got: serde_json::Value = lookup(&bytes, "192.0.2.1").unwrap();
    let want = serde_json::json!({
        "tiny": -5,
        "small": -300,
        "letter": "x",
        "depth": 42,                                  // newtype struct is transparent
        "pair": [7, "seven"],                        // tuple struct → array
        "tuple": [1, "one"],                         // tuple → array
        "newtype_variant": {"Point": 9},             // externally tagged
        "tuple_variant": {"Line": [3, 4]},
        "struct_variant": {"Rect": {"w": 10, "h": 20}},
    });
    assert_eq!(got, want);
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
