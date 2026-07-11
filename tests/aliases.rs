//! IPv4 aliasing: 6to4 / Teredo / IPv4-mapped queries resolve to IPv4 data, the option can
//! be disabled, and inserting into aliased space is rejected.

mod common;

use ipnet::IpNet;
use mmdb_writer::{Error, Ipv4Aliasing, Value, Writer};

use common::lookup;

fn net(s: &str) -> IpNet {
    s.parse().expect("valid CIDR")
}

#[test]
fn aliased_queries_resolve_to_ipv4_data() {
    let mut w = Writer::new("Alias");
    w.insert_value(net("1.2.3.0/24"), Value::from(99_u32))
        .unwrap();
    let bytes = w.to_bytes().unwrap();

    // Direct IPv4.
    assert_eq!(lookup::<u32>(&bytes, "1.2.3.4"), Some(99));
    // IPv4-mapped IPv6 (::ffff:1.2.3.4).
    assert_eq!(lookup::<u32>(&bytes, "::ffff:1.2.3.4"), Some(99));
    // 6to4 (2002:0102:0304::) embeds 1.2.3.4 in bits 16..48.
    assert_eq!(lookup::<u32>(&bytes, "2002:102:304::"), Some(99));
    // Teredo (2001:0:0102:0304::) embeds 1.2.3.4 in bits 32..64.
    assert_eq!(lookup::<u32>(&bytes, "2001:0:102:304::"), Some(99));
}

#[test]
fn disabling_aliasing_makes_6to4_miss() {
    let mut w = Writer::builder("Alias")
        .ipv4_aliasing(Ipv4Aliasing::Disabled)
        .build();
    w.insert_value(net("1.2.3.0/24"), Value::from(99_u32))
        .unwrap();
    let bytes = w.to_bytes().unwrap();

    // Direct IPv4 still resolves...
    assert_eq!(lookup::<u32>(&bytes, "1.2.3.4"), Some(99));
    // ...but the 6to4 alias is gone.
    assert_eq!(lookup::<u32>(&bytes, "2002:102:304::"), None);
}

#[test]
fn inserting_into_aliased_space_is_rejected() {
    let mut w = Writer::new("Alias");
    assert!(matches!(
        w.insert_value(net("2002::/16"), Value::from(1_u32)),
        Err(Error::AliasedNetwork(_))
    ));
    assert!(matches!(
        w.insert_value(net("::ffff:1.2.3.0/120"), Value::from(1_u32)),
        Err(Error::AliasedNetwork(_))
    ));
    assert!(matches!(
        w.insert_value(net("2001::/32"), Value::from(1_u32)),
        Err(Error::AliasedNetwork(_))
    ));
}

#[test]
fn disabled_aliasing_allows_inserting_into_those_ranges() {
    let mut w = Writer::builder("Alias")
        .ipv4_aliasing(Ipv4Aliasing::Disabled)
        .build();
    // With aliasing off, 2002::/16 is ordinary IPv6 space.
    w.insert_value(net("2002::/16"), Value::from(5_u32))
        .unwrap();
    let bytes = w.to_bytes().unwrap();
    assert_eq!(lookup::<u32>(&bytes, "2002:1::"), Some(5));
}
