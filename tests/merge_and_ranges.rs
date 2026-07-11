//! Tests for `insert_with`, the merge strategies, `insert_range`, and `get`.

mod common;

use std::collections::BTreeMap;

use ipnet::IpNet;
use mmdb_writer::{MergeStrategy, Value, Writer};

use common::lookup;

fn net(s: &str) -> IpNet {
    s.parse().expect("valid CIDR")
}

#[test]
fn top_level_merge_keeps_sibling_keys() {
    let mut w = Writer::new("Merge");
    w.insert_value(net("10.0.0.0/24"), Value::map([("a", Value::from(1_u32))]))
        .unwrap();
    w.insert_value_merged(
        net("10.0.0.0/24"),
        Value::map([("b", Value::from(2_u32))]),
        MergeStrategy::TopLevelMerge,
    )
    .unwrap();
    let bytes = w.to_bytes().unwrap();

    let m: BTreeMap<String, u32> = lookup(&bytes, "10.0.0.5").unwrap();
    assert_eq!(m.get("a"), Some(&1));
    assert_eq!(m.get("b"), Some(&2));
}

#[test]
fn deep_merge_recurses_into_nested_maps() {
    #[derive(serde::Deserialize, Debug, PartialEq)]
    struct Outer {
        info: Inner,
    }
    #[derive(serde::Deserialize, Debug, PartialEq)]
    struct Inner {
        x: u32,
        y: u32,
    }

    let mut w = Writer::new("Merge");
    w.insert_value(
        net("10.0.0.0/24"),
        Value::map([("info", Value::map([("x", Value::from(1_u32))]))]),
    )
    .unwrap();
    w.insert_value_merged(
        net("10.0.0.0/24"),
        Value::map([("info", Value::map([("y", Value::from(2_u32))]))]),
        MergeStrategy::DeepMerge,
    )
    .unwrap();
    let bytes = w.to_bytes().unwrap();

    assert_eq!(
        lookup::<Outer>(&bytes, "10.0.0.5"),
        Some(Outer {
            info: Inner { x: 1, y: 2 }
        })
    );
}

#[test]
fn insert_with_sees_existing_and_can_accumulate() {
    let mut w = Writer::new("Counter");
    let bump = |w: &mut Writer, net_str: &str| {
        w.insert_with(net(net_str), |existing| {
            let n = match existing {
                Some(Value::U32(n)) => *n + 1,
                _ => 1,
            };
            Some(Value::from(n))
        })
        .unwrap();
    };
    bump(&mut w, "192.0.2.0/24");
    bump(&mut w, "192.0.2.0/25"); // overlaps lower half, sees 1 → 2
    let bytes = w.to_bytes().unwrap();

    // Lower half (in the /25) was bumped twice; upper half only once.
    assert_eq!(lookup::<u32>(&bytes, "192.0.2.10"), Some(2));
    assert_eq!(lookup::<u32>(&bytes, "192.0.2.200"), Some(1));
}

#[test]
fn insert_with_none_clears_a_leaf() {
    let mut w = Writer::new("Removal");
    w.insert_value(net("10.0.0.0/24"), Value::from(1_u32))
        .unwrap();
    // Clear the lower /25.
    w.insert_with(net("10.0.0.0/25"), |_| None).unwrap();
    let bytes = w.to_bytes().unwrap();

    assert_eq!(lookup::<u32>(&bytes, "10.0.0.10"), None); // cleared
    assert_eq!(lookup::<u32>(&bytes, "10.0.0.200"), Some(1)); // untouched upper half
}

#[test]
fn insert_range_covers_exactly_the_range() {
    let mut w = Writer::new("Range");
    w.insert_range(
        "1.1.1.1".parse().unwrap(),
        "1.1.1.2".parse().unwrap(),
        &Value::from(42_u32),
    )
    .unwrap();
    let bytes = w.to_bytes().unwrap();

    assert_eq!(lookup::<u32>(&bytes, "1.1.1.0"), None);
    assert_eq!(lookup::<u32>(&bytes, "1.1.1.1"), Some(42));
    assert_eq!(lookup::<u32>(&bytes, "1.1.1.2"), Some(42));
    assert_eq!(lookup::<u32>(&bytes, "1.1.1.3"), None);
}

#[test]
fn get_reflects_current_state() {
    let mut w = Writer::new("Get");
    assert_eq!(w.get("10.0.0.5".parse().unwrap()), None);
    w.insert_value(net("10.0.0.0/24"), Value::from(1_u32))
        .unwrap();
    assert_eq!(
        w.get("10.0.0.5".parse().unwrap()),
        Some(&Value::from(1_u32))
    );
    assert_eq!(w.get("10.0.1.5".parse().unwrap()), None);
}
