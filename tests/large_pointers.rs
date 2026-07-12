//! End-to-end coverage of the larger pointer size classes: build a database whose data
//! section grows past the 2 KiB and 514 KiB pointer-class boundaries, with values shared
//! across networks whose first emission sits beyond each boundary — then verify the real
//! reader follows those pointers.

mod common;

use std::net::Ipv4Addr;

use ipnet::{IpNet, Ipv4Net};
use mmdb_writer::{Value, Writer};

use common::lookup;

/// A unique ~1 KiB filler value for network #i.
fn filler(i: u32) -> Value {
    Value::map([
        ("id", Value::from(i)),
        ("pad", Value::from(format!("{i:08x}").repeat(128).as_str())),
    ])
}

fn nth_net(i: u32) -> IpNet {
    // 10.x.y.0/24 for i, disjoint.
    IpNet::V4(Ipv4Net::new(Ipv4Addr::from(0x0A00_0000 + (i << 8)), 24).unwrap())
}

#[derive(serde::Deserialize, Debug, PartialEq)]
struct Marker {
    marker: String,
}

#[derive(serde::Deserialize)]
struct Filler {
    id: u32,
    pad: String,
}

#[test]
fn values_behind_three_and_four_byte_pointers_resolve() {
    let mut w = Writer::new("Big-Pointers");

    // ~700 unique 1 KiB fillers push the data section well past 526_336 bytes.
    for i in 0..700 {
        w.insert_value(nth_net(i), filler(i)).unwrap();
    }

    // A value first emitted after the 2 KiB boundary, then shared by a second network: the
    // second reference becomes a 3-byte-class pointer.
    let mid = Value::map([("marker", Value::from("after-2k"))]);
    w.insert_value(nth_net(800), mid.clone()).unwrap();

    // And one first emitted after the 514 KiB boundary → 4-byte-class pointers.
    let late = Value::map([("marker", Value::from("after-514k"))]);
    w.insert_value(nth_net(801), late.clone()).unwrap();

    // Share both values across more networks. Their map *values* dedup via pointers back to
    // the first emission offsets, which lie beyond the class boundaries.
    for i in 0..8 {
        w.insert_value(nth_net(900 + i), mid.clone()).unwrap();
        w.insert_value(nth_net(910 + i), late.clone()).unwrap();
    }

    let bytes = w.to_bytes().unwrap();
    assert!(
        bytes.len() > 600_000,
        "data section must cross the 4-byte pointer boundary (got {} bytes)",
        bytes.len()
    );

    // Spot-check fillers across the whole range.
    for i in [0u32, 137, 350, 699] {
        let ip = format!("10.{}.{}.9", (i >> 8) & 0xFF, i & 0xFF);
        let got: Filler = lookup(&bytes, &ip).unwrap();
        assert_eq!(got.id, i);
        assert_eq!(got.pad.len(), 1024);
    }
    // Every shared-value network resolves through its large-offset pointer.
    for i in 0..8u32 {
        let n = 900 + i;
        let ip = format!("10.{}.{}.9", (n >> 8) & 0xFF, n & 0xFF);
        assert_eq!(
            lookup::<Marker>(&bytes, &ip),
            Some(Marker {
                marker: "after-2k".into()
            }),
            "network sharing the post-2KiB value must resolve at {ip}"
        );
        let n = 910 + i;
        let ip = format!("10.{}.{}.9", (n >> 8) & 0xFF, n & 0xFF);
        assert_eq!(
            lookup::<Marker>(&bytes, &ip),
            Some(Marker {
                marker: "after-514k".into()
            }),
            "network sharing the post-514KiB value must resolve at {ip}"
        );
    }
}
