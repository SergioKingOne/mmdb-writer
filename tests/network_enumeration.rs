//! Exhaustive verification: enumerate every network the reader sees in a written database
//! and compare the exact set of (network, value) pairs against what was inserted.

mod common;

use ipnet::IpNet;
use maxminddb::WithinOptions;
use mmdb_writer::{Ipv4Aliasing, Value, Writer};

use common::reader;

fn net(s: &str) -> IpNet {
    s.parse().expect("valid CIDR")
}

/// Collect every (network, id) pair the reader can enumerate from the database.
fn enumerate(bytes: &[u8]) -> Vec<(String, u32)> {
    let r = reader(bytes);
    let mut out = Vec::new();
    for item in r.networks(WithinOptions::default()).unwrap() {
        let item = item.unwrap();
        let id: Option<u32> = item.decode().unwrap();
        out.push((item.network().unwrap().to_string(), id.unwrap()));
    }
    out.sort();
    out
}

#[test]
fn disjoint_networks_enumerate_exactly() {
    let mut w = Writer::builder("Enum")
        .ipv4_aliasing(Ipv4Aliasing::Disabled)
        .build();
    let inserted = [
        ("1.0.0.0/24", 1_u32),
        ("2.0.0.0/16", 2),
        ("3.3.3.3/32", 3),
        ("2001:db8::/64", 4),
    ];
    for (cidr, id) in inserted {
        w.insert_value(net(cidr), Value::from(id)).unwrap();
    }
    let got = enumerate(&w.to_bytes().unwrap());

    // The reader reports IPv4 networks embedded in the v6 tree in native dotted-quad form.
    let want: Vec<(String, u32)> = vec![
        ("1.0.0.0/24".into(), 1),
        ("2.0.0.0/16".into(), 2),
        ("3.3.3.3/32".into(), 3),
        ("2001:db8::/64".into(), 4),
    ];
    let mut want_sorted = want;
    want_sorted.sort();
    assert_eq!(got, want_sorted);
}

#[test]
fn split_networks_reassemble_exactly() {
    // Insert a /24, then override its lower /25: enumeration must show exactly the /25 with
    // the new value and the upper /25 with the old value — no gaps, no extras.
    let mut w = Writer::builder("Enum")
        .ipv4_aliasing(Ipv4Aliasing::Disabled)
        .build();
    w.insert_value(net("9.9.9.0/24"), Value::from(1_u32))
        .unwrap();
    w.insert_value(net("9.9.9.0/25"), Value::from(2_u32))
        .unwrap();
    let got = enumerate(&w.to_bytes().unwrap());

    let mut want: Vec<(String, u32)> = vec![("9.9.9.0/25".into(), 2), ("9.9.9.128/25".into(), 1)];
    want.sort();
    assert_eq!(got, want);
}

#[test]
fn adjacent_same_value_networks_merge_in_enumeration() {
    // Two adjacent /25s with the same value must collapse into one /24 on disk (tree
    // compaction), which the enumeration reflects.
    let mut w = Writer::builder("Enum")
        .ipv4_aliasing(Ipv4Aliasing::Disabled)
        .build();
    w.insert_value(net("7.7.7.0/25"), Value::from(1_u32))
        .unwrap();
    w.insert_value(net("7.7.7.128/25"), Value::from(1_u32))
        .unwrap();
    let got = enumerate(&w.to_bytes().unwrap());

    assert_eq!(got, vec![("7.7.7.0/24".to_string(), 1)]);
}
