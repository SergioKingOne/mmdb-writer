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
        let mut w = Writer::new("Prop");
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
}
