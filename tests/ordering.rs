//! Insert-ordering correctness: nested prefixes resolve most-specific-first, and the output
//! is independent of the order non-overlapping prefixes are inserted.

mod common;

use std::time::SystemTime;

use ipnet::IpNet;
use mmdb_writer::{Value, Writer};

use common::lookup;

fn net(s: &str) -> IpNet {
    s.parse().expect("valid CIDR")
}

#[test]
fn nested_prefixes_resolve_most_specific() {
    let mut w = Writer::new("Nested");
    w.insert_value(net("10.0.0.0/8"), Value::from(8_u32))
        .unwrap();
    w.insert_value(net("10.1.0.0/16"), Value::from(16_u32))
        .unwrap();
    w.insert_value(net("10.1.2.0/24"), Value::from(24_u32))
        .unwrap();
    let bytes = w.to_bytes().unwrap();

    assert_eq!(lookup::<u32>(&bytes, "10.1.2.3"), Some(24)); // most specific
    assert_eq!(lookup::<u32>(&bytes, "10.1.9.9"), Some(16)); // /16 but not /24
    assert_eq!(lookup::<u32>(&bytes, "10.9.9.9"), Some(8)); // /8 only
    assert_eq!(lookup::<u32>(&bytes, "11.0.0.1"), None); // outside
}

#[test]
fn less_specific_inserted_last_paints_over() {
    let mut w = Writer::new("Paint");
    w.insert_value(net("10.1.2.0/24"), Value::from(24_u32))
        .unwrap();
    // A broader network inserted afterward overwrites the whole range.
    w.insert_value(net("10.0.0.0/8"), Value::from(8_u32))
        .unwrap();
    let bytes = w.to_bytes().unwrap();

    assert_eq!(lookup::<u32>(&bytes, "10.1.2.3"), Some(8));
}

#[test]
fn insertion_order_of_disjoint_prefixes_does_not_affect_output() {
    let epoch = SystemTime::UNIX_EPOCH;
    let entries = [
        ("1.0.0.0/24", 1_u32),
        ("2.0.0.0/24", 2),
        ("3.0.0.0/16", 3),
        ("4.4.0.0/16", 4),
        ("5.5.5.0/24", 5),
    ];

    let forward = {
        let mut w = Writer::builder("Order").build_epoch(epoch).build();
        for (cidr, v) in entries {
            w.insert_value(net(cidr), Value::from(v)).unwrap();
        }
        w.to_bytes().unwrap()
    };
    let reversed = {
        let mut w = Writer::builder("Order").build_epoch(epoch).build();
        for (cidr, v) in entries.iter().rev() {
            w.insert_value(net(cidr), Value::from(*v)).unwrap();
        }
        w.to_bytes().unwrap()
    };

    // Disjoint prefixes: order must not change the serialized bytes.
    assert_eq!(forward, reversed);
}
