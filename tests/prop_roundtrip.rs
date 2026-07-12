//! Property-based round trips: for arbitrary sets of host routes, every inserted address
//! reads back its value through the `maxminddb` reader.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use ipnet::IpNet;
use maxminddb::Reader;
use mmdb_writer::{Value, Writer};
use proptest::prelude::*;

proptest! {
    #[test]
    fn ipv4_host_routes_round_trip(
        map in prop::collection::btree_map(any::<u32>(), any::<u32>(), 1..40)
    ) {
        let mut w = Writer::new("Prop");
        for (&ip, &val) in &map {
            let network = IpNet::from(IpAddr::V4(Ipv4Addr::from(ip)));
            w.insert_value(network, Value::from(val)).unwrap();
        }
        let bytes = w.to_bytes().unwrap();
        let reader = Reader::from_source(&bytes[..]).unwrap();

        for (&ip, &val) in &map {
            let addr = IpAddr::V4(Ipv4Addr::from(ip));
            let got: Option<u32> = reader.lookup(addr).unwrap().decode().unwrap();
            prop_assert_eq!(got, Some(val), "address {} should resolve to its value", addr);
        }
    }

    #[test]
    fn ipv6_host_routes_round_trip(
        map in prop::collection::btree_map(any::<u128>(), any::<u32>(), 1..40)
    ) {
        // Random u128s can land inside the IPv4-aliased ranges (::ffff:0:0/96, 2001::/32,
        // 2002::/16), which a default writer rejects by design — so disable aliasing here;
        // `aliased_inserts_always_error_cleanly` covers the rejection path.
        let mut w = Writer::builder("Prop")
            .ipv4_aliasing(mmdb_writer::Ipv4Aliasing::Disabled)
            .build();
        for (&ip, &val) in &map {
            let network = IpNet::from(IpAddr::V6(Ipv6Addr::from(ip)));
            w.insert_value(network, Value::from(val)).unwrap();
        }
        let bytes = w.to_bytes().unwrap();
        let reader = Reader::from_source(&bytes[..]).unwrap();

        for (&ip, &val) in &map {
            let addr = IpAddr::V6(Ipv6Addr::from(ip));
            let got: Option<u32> = reader.lookup(addr).unwrap().decode().unwrap();
            prop_assert_eq!(got, Some(val), "address {} should resolve to its value", addr);
        }
    }

    #[test]
    fn aliased_inserts_always_error_cleanly(offset in any::<u128>(), val in any::<u32>()) {
        // Any address inside 2002::/16 must be rejected by a default (aliasing-enabled)
        // writer with the AliasedNetwork error — never a panic or silent corruption.
        let ip = 0x2002_0000_0000_0000_0000_0000_0000_0000u128 | (offset >> 16);
        let mut w = Writer::new("Prop");
        let result = w.insert_value(
            IpNet::from(IpAddr::V6(Ipv6Addr::from(ip))),
            Value::from(val),
        );
        prop_assert!(matches!(result, Err(mmdb_writer::Error::AliasedNetwork(_))));
        // The writer is still usable and produces a valid database afterward.
        w.insert_value("10.0.0.0/24".parse::<IpNet>().unwrap(), Value::from(val)).unwrap();
        let bytes = w.to_bytes().unwrap();
        prop_assert!(Reader::from_source(&bytes[..]).is_ok());
    }
}
