//! Build a GeoIP-style database from `#[derive(Serialize)]` records and write it to a file.
//! Read the result back with the [`maxminddb`](https://crates.io/crates/maxminddb) crate.
//!
//! Run with: `cargo run --example serde_structs`

use std::collections::BTreeMap;
use std::error::Error;

use ipnet::IpNet;
use mmdb_writer::Writer;
use serde::Serialize;

#[derive(Serialize)]
struct City {
    names: BTreeMap<String, String>,
    geoname_id: u32,
    is_in_european_union: bool,
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut writer = Writer::new("Example-City-DB");

    let names = BTreeMap::from([
        ("en".to_string(), "Example City".to_string()),
        ("de".to_string(), "Beispielstadt".to_string()),
    ]);
    writer.insert(
        "81.2.69.0/24".parse::<IpNet>()?,
        &City {
            names,
            geoname_id: 2_643_743,
            is_in_european_union: false,
        },
    )?;

    writer.write_to(std::fs::File::create("example-city.mmdb")?)?;
    println!("wrote example-city.mmdb — read it back with the `maxminddb` crate");
    Ok(())
}
