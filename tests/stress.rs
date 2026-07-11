//! Volume test: build a database of many /24 networks and confirm it stays correct. The
//! large variant is `#[ignore]`d by default; run with `cargo test -- --ignored`.

use std::net::{IpAddr, Ipv4Addr};

use ipnet::{IpNet, Ipv4Net};
use maxminddb::Reader;
use mmdb_writer::{Value, Writer};

fn run(count: u32) {
    let mut w = Writer::new("Stress");
    for i in 0..count {
        // `/24` whose base address is `i << 8`, tagged with the value `i`.
        let base = Ipv4Addr::from(i << 8);
        let network = IpNet::V4(Ipv4Net::new(base, 24).unwrap());
        w.insert_value(network, Value::from(i)).unwrap();
    }
    let bytes = w.to_bytes().unwrap();
    let reader = Reader::from_source(&bytes[..]).expect("stress database is readable");

    // Spot-check a spread of the inserted networks (a host inside each `/24`).
    let mut i = 0;
    while i < count {
        let host = IpAddr::V4(Ipv4Addr::from((i << 8) + 5));
        let got: Option<u32> = reader.lookup(host).unwrap().decode().unwrap();
        assert_eq!(got, Some(i), "host in network #{i} should resolve to {i}");
        i += 97;
    }
}

#[test]
fn stress_moderate() {
    run(5_000);
}

#[test]
#[ignore = "large; run with --ignored"]
fn stress_large() {
    run(200_000);
}
