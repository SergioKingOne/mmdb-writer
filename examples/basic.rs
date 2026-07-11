//! Build a small database with the `Value` API (no serde required) and write it to a file.
//!
//! Run with: `cargo run --example basic`

use std::error::Error;

use ipnet::IpNet;
use mmdb_writer::{Value, Writer};

fn main() -> Result<(), Box<dyn Error>> {
    let mut writer = Writer::new("Example-ASN-DB");

    writer.insert_value(
        "192.0.2.0/24".parse::<IpNet>()?,
        Value::map([
            ("autonomous_system_number", Value::from(64_512_u32)),
            (
                "autonomous_system_organization",
                Value::from("Example, Inc."),
            ),
        ]),
    )?;

    writer.insert_value(
        "2001:db8::/32".parse::<IpNet>()?,
        Value::map([("autonomous_system_number", Value::from(65_000_u32))]),
    )?;

    let bytes = writer.to_bytes()?;
    std::fs::write("example-asn.mmdb", &bytes)?;
    println!("wrote example-asn.mmdb ({} bytes)", bytes.len());
    Ok(())
}
