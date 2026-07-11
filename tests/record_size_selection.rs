//! Record-size auto-selection and explicit override.

mod common;

use ipnet::IpNet;
use mmdb_writer::{RecordSize, Value, Writer};

use common::reader;

fn net(s: &str) -> IpNet {
    s.parse().expect("valid CIDR")
}

#[test]
fn small_database_auto_selects_24_bit_records() {
    let mut w = Writer::new("Auto");
    w.insert_value(net("1.2.3.0/24"), Value::from(1_u32))
        .unwrap();
    let bytes = w.to_bytes().unwrap();
    assert_eq!(reader(&bytes).metadata().record_size, 24);
}

#[test]
fn explicit_record_size_is_honored() {
    for (size, expected) in [
        (RecordSize::Bits24, 24),
        (RecordSize::Bits28, 28),
        (RecordSize::Bits32, 32),
    ] {
        let mut w = Writer::builder("Explicit").record_size(size).build();
        w.insert_value(net("1.2.3.0/24"), Value::from(1_u32))
            .unwrap();
        let bytes = w.to_bytes().unwrap();
        assert_eq!(reader(&bytes).metadata().record_size, expected);
    }
}
