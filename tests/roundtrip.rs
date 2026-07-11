//! End-to-end round trips: write with `mmdb_writer`, read back with `maxminddb`.

mod common;

use std::time::{Duration, SystemTime};

use ipnet::IpNet;
use mmdb_writer::{IpVersion, RecordSize, Value, Writer};
use serde::Deserialize;

use common::{lookup, reader};

#[derive(Debug, PartialEq, Deserialize)]
struct Record {
    name: String,
    n: u32,
}

fn net(s: &str) -> IpNet {
    s.parse().expect("valid CIDR")
}

fn sample_value(name: &str, n: u32) -> Value {
    Value::map([("name", Value::from(name)), ("n", Value::from(n))])
}

#[test]
fn single_ipv4_network_round_trips() {
    let mut w = Writer::new("Round-Trip-Test");
    w.insert_value(net("1.2.3.0/24"), sample_value("alpha", 42))
        .unwrap();
    let bytes = w.to_bytes().unwrap();

    assert_eq!(
        lookup::<Record>(&bytes, "1.2.3.5"),
        Some(Record {
            name: "alpha".into(),
            n: 42
        })
    );
    // Both ends of the /24 resolve.
    assert!(lookup::<Record>(&bytes, "1.2.3.0").is_some());
    assert!(lookup::<Record>(&bytes, "1.2.3.255").is_some());
    // Outside the network is a miss.
    assert_eq!(lookup::<Record>(&bytes, "1.2.4.0"), None);
}

#[test]
fn ipv6_network_round_trips() {
    let mut w = Writer::new("Round-Trip-Test");
    w.insert_value(net("2001:db8::/32"), sample_value("v6", 7))
        .unwrap();
    let bytes = w.to_bytes().unwrap();

    assert_eq!(
        lookup::<Record>(&bytes, "2001:db8::1"),
        Some(Record {
            name: "v6".into(),
            n: 7
        })
    );
    assert_eq!(lookup::<Record>(&bytes, "2001:db9::1"), None);
}

#[test]
fn exhaustive_small_cidr_scan() {
    let mut w = Writer::new("Round-Trip-Test");
    w.insert_value(net("10.0.0.0/30"), sample_value("block", 1))
        .unwrap();
    let bytes = w.to_bytes().unwrap();

    for host in 0..4 {
        assert!(
            lookup::<Record>(&bytes, &format!("10.0.0.{host}")).is_some(),
            "10.0.0.{host} should be covered by 10.0.0.0/30"
        );
    }
    assert_eq!(lookup::<Record>(&bytes, "10.0.0.4"), None);
}

#[test]
fn metadata_fields_are_written() {
    let epoch = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    let mut w = Writer::builder("Meta-Test")
        .languages(["en", "de"])
        .build_epoch(epoch)
        .build();
    w.insert_value(net("192.0.2.0/24"), sample_value("m", 1))
        .unwrap();
    let bytes = w.to_bytes().unwrap();

    let reader = reader(&bytes);
    let meta = reader.metadata();
    assert_eq!(meta.database_type, "Meta-Test");
    assert_eq!(meta.ip_version, 6);
    assert_eq!(meta.binary_format_major_version, 2);
    assert_eq!(meta.binary_format_minor_version, 0);
    assert_eq!(meta.build_epoch, 1_700_000_000);
    assert_eq!(meta.languages, vec!["en".to_string(), "de".to_string()]);
    assert!(meta.node_count > 0);
    assert!([24, 28, 32].contains(&meta.record_size));
}

#[test]
fn output_is_deterministic_with_pinned_epoch() {
    let epoch = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    let build = || {
        let mut w = Writer::builder("Deterministic")
            .build_epoch(epoch)
            .record_size(RecordSize::Bits28)
            .build();
        w.insert_value(net("1.0.0.0/24"), sample_value("a", 1))
            .unwrap();
        w.insert_value(net("2.0.0.0/24"), sample_value("b", 2))
            .unwrap();
        w.to_bytes().unwrap()
    };
    assert_eq!(build(), build());
}

#[test]
fn ipv4_only_database_round_trips() {
    let mut w = Writer::builder("V4-Only").ip_version(IpVersion::V4).build();
    w.insert_value(net("203.0.113.0/24"), sample_value("v4", 9))
        .unwrap();
    let bytes = w.to_bytes().unwrap();

    let reader = reader(&bytes);
    assert_eq!(reader.metadata().ip_version, 4);
    assert_eq!(
        lookup::<Record>(&bytes, "203.0.113.50"),
        Some(Record {
            name: "v4".into(),
            n: 9
        })
    );
}
