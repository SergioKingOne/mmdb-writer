//! Edge cases: empty databases, host routes, whole-address-space inserts, and exact
//! network-boundary agreement between the writer and the reader.

mod common;

use std::net::IpAddr;

use ipnet::IpNet;
use mmdb_writer::{IpVersion, Value, Writer};

use common::{lookup, reader};

fn net(s: &str) -> IpNet {
    s.parse().expect("valid CIDR")
}

#[test]
fn empty_database_is_valid_and_all_lookups_miss() {
    // A writer with zero inserts must still produce a database readers accept.
    let bytes = Writer::new("Empty").to_bytes().unwrap();
    let r = reader(&bytes);
    assert_eq!(r.metadata().database_type, "Empty");
    assert_eq!(lookup::<u32>(&bytes, "8.8.8.8"), None);
    assert_eq!(lookup::<u32>(&bytes, "2001:db8::1"), None);
}

#[test]
fn empty_ipv4_database_is_valid() {
    let bytes = Writer::builder("Empty-V4")
        .ip_version(IpVersion::V4)
        .build()
        .to_bytes()
        .unwrap();
    assert_eq!(lookup::<u32>(&bytes, "8.8.8.8"), None);
}

#[test]
fn single_host_routes_round_trip() {
    let mut w = Writer::new("Hosts");
    w.insert_value(net("203.0.113.7/32"), Value::from(4_u32))
        .unwrap();
    w.insert_value(net("2001:db8::42/128"), Value::from(6_u32))
        .unwrap();
    let bytes = w.to_bytes().unwrap();

    assert_eq!(lookup::<u32>(&bytes, "203.0.113.7"), Some(4));
    assert_eq!(lookup::<u32>(&bytes, "203.0.113.6"), None);
    assert_eq!(lookup::<u32>(&bytes, "203.0.113.8"), None);
    assert_eq!(lookup::<u32>(&bytes, "2001:db8::42"), Some(6));
    assert_eq!(lookup::<u32>(&bytes, "2001:db8::41"), None);
    assert_eq!(lookup::<u32>(&bytes, "2001:db8::43"), None);
}

#[test]
fn whole_ipv4_space_via_zero_prefix() {
    let mut w = Writer::builder("All-V4").ip_version(IpVersion::V4).build();
    w.insert_value(net("0.0.0.0/0"), Value::from(1_u32))
        .unwrap();
    let bytes = w.to_bytes().unwrap();

    for ip in ["0.0.0.0", "8.8.8.8", "127.0.0.1", "255.255.255.255"] {
        assert_eq!(lookup::<u32>(&bytes, ip), Some(1), "{ip} should match ::/0");
    }
}

#[test]
fn whole_ipv6_space_via_zero_prefix() {
    let mut w = Writer::builder("All-V6")
        .ipv4_aliasing(mmdb_writer::Ipv4Aliasing::Disabled)
        .build();
    w.insert_value(net("::/0"), Value::from(1_u32)).unwrap();
    let bytes = w.to_bytes().unwrap();

    for ip in [
        "::",
        "2001:db8::1",
        "ffff:ffff:ffff:ffff:ffff:ffff:ffff:ffff",
    ] {
        assert_eq!(lookup::<u32>(&bytes, ip), Some(1), "{ip} should match ::/0");
    }
}

#[test]
fn host_bits_are_truncated() {
    let mut w = Writer::new("Trunc");
    // 10.0.0.99/24 has host bits set; must behave as 10.0.0.0/24.
    w.insert_value(net("10.0.0.99/24"), Value::from(1_u32))
        .unwrap();
    let bytes = w.to_bytes().unwrap();
    assert_eq!(lookup::<u32>(&bytes, "10.0.0.1"), Some(1));
    assert_eq!(lookup::<u32>(&bytes, "10.0.0.254"), Some(1));
    assert_eq!(lookup::<u32>(&bytes, "10.0.1.1"), None);
}

#[test]
fn writer_get_agrees_with_reader_across_a_full_sweep() {
    // Build an overlapping layout, then compare writer.get() with the real reader for every
    // address in a /24 sweep — the writer's view and the on-disk truth must agree exactly.
    let mut w = Writer::new("Sweep");
    w.insert_value(net("198.18.5.0/24"), Value::from(24_u32))
        .unwrap();
    w.insert_value(net("198.18.5.64/26"), Value::from(26_u32))
        .unwrap();
    w.insert_value(net("198.18.5.96/27"), Value::from(27_u32))
        .unwrap();
    w.insert_with(net("198.18.5.192/26"), |_| None).unwrap(); // punch a hole
    let bytes = w.to_bytes().unwrap();
    let r = reader(&bytes);

    for host in 0..=255u32 {
        let ip: IpAddr = format!("198.18.5.{host}").parse().unwrap();
        let from_reader: Option<u32> = r.lookup(ip).unwrap().decode().unwrap();
        let from_writer = match w.get(ip) {
            Some(Value::U32(n)) => Some(*n),
            None => None,
            other => panic!("unexpected value {other:?}"),
        };
        assert_eq!(
            from_writer, from_reader,
            "writer.get and reader disagree at {ip}"
        );
    }
}

#[test]
fn deeply_nested_values_round_trip() {
    // A 12-level-deep nested map exercises recursive encoding.
    let mut value = Value::from("leaf");
    for depth in 0..12 {
        value = Value::map([(format!("level{depth}"), value)]);
    }
    let mut w = Writer::new("Deep");
    w.insert_value(net("10.0.0.0/24"), value.clone()).unwrap();
    let bytes = w.to_bytes().unwrap();

    // Walk back down through the decoded maps.
    let mut decoded: serde_json::Value = serde_json::to_value(
        lookup::<std::collections::BTreeMap<String, serde_json::Value>>(&bytes, "10.0.0.1")
            .unwrap(),
    )
    .unwrap();
    for depth in (0..12).rev() {
        decoded = decoded
            .get(format!("level{depth}"))
            .cloned()
            .unwrap_or_else(|| panic!("missing level{depth}"));
    }
    assert_eq!(decoded, serde_json::Value::String("leaf".into()));
}

#[test]
#[cfg(feature = "serde")]
fn large_strings_and_all_value_types_round_trip() {
    #[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
    struct Everything {
        s: String,
        big_s: String,
        b: bool,
        f32_: f32,
        f64_: f64,
        u16_: u16,
        u32_: u32,
        u64_: u64,
        u128_: u128,
        i32_neg: i32,
        arr: Vec<u32>,
        bytes: serde_bytes::ByteBuf,
    }
    let rec = Everything {
        s: "small".into(),
        big_s: "x".repeat(70_000), // forces the 3-byte size-class path
        b: true,
        f32_: 1.25,
        f64_: -2.5e300,
        u16_: u16::MAX,
        u32_: u32::MAX,
        u64_: u64::MAX,
        u128_: u128::MAX,
        i32_neg: i32::MIN,
        arr: vec![1, 2, 3],
        bytes: serde_bytes::ByteBuf::from(vec![0u8, 255, 128]),
    };
    let mut w = Writer::new("Everything");
    w.insert(net("10.0.0.0/24"), &rec).unwrap();
    let bytes = w.to_bytes().unwrap();
    assert_eq!(lookup::<Everything>(&bytes, "10.0.0.1"), Some(rec));
}
