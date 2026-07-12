//! Reserved-network handling and the metadata-pointer option.

mod common;

use ipnet::IpNet;
use mmdb_writer::{Error, MetadataPointers, ReservedNetworks, Value, Writer};

use common::{lookup, reader};

fn net(s: &str) -> IpNet {
    s.parse().expect("valid CIDR")
}

#[test]
fn reserved_included_by_default_allows_reserved_inserts() {
    let mut w = Writer::new("Reserved");
    // 10.0.0.0/8 is reserved but permitted by default.
    w.insert_value(net("10.0.0.0/8"), Value::from(1_u32))
        .unwrap();
    let bytes = w.to_bytes().unwrap();
    assert_eq!(lookup::<u32>(&bytes, "10.1.2.3"), Some(1));
}

#[test]
fn reserved_excluded_rejects_reserved_inserts() {
    let mut w = Writer::builder("Reserved")
        .reserved_networks(ReservedNetworks::Excluded)
        .build();
    assert!(matches!(
        w.insert_value(net("10.0.0.0/8"), Value::from(1_u32)),
        Err(Error::ReservedNetwork(_))
    ));
    // A subnet of a reserved network is also rejected.
    assert!(matches!(
        w.insert_value(net("192.168.1.0/24"), Value::from(1_u32)),
        Err(Error::ReservedNetwork(_))
    ));
    // An IPv6 reserved range too.
    assert!(matches!(
        w.insert_value(net("fc00::/7"), Value::from(1_u32)),
        Err(Error::ReservedNetwork(_))
    ));
}

#[test]
fn reserved_excluded_carves_reserved_out_of_broader_insert() {
    let mut w = Writer::builder("Reserved")
        .reserved_networks(ReservedNetworks::Excluded)
        .build();
    // A default route over all of IPv4 — reserved ranges must be carved out.
    w.insert_value(net("0.0.0.0/0"), Value::from(7_u32))
        .unwrap();
    let bytes = w.to_bytes().unwrap();

    // Public address gets data.
    assert_eq!(lookup::<u32>(&bytes, "8.8.8.8"), Some(7));
    // Reserved addresses are carved out (no data).
    assert_eq!(lookup::<u32>(&bytes, "10.1.2.3"), None);
    assert_eq!(lookup::<u32>(&bytes, "127.0.0.1"), None);
    assert_eq!(lookup::<u32>(&bytes, "192.168.0.1"), None);
}

#[test]
fn non_reserved_insert_works_with_exclusion_enabled() {
    let mut w = Writer::builder("Reserved")
        .reserved_networks(ReservedNetworks::Excluded)
        .build();
    // 8.8.8.0/24 is public; inserting it is fine even with exclusion on.
    w.insert_value(net("8.8.8.0/24"), Value::from(3_u32))
        .unwrap();
    let bytes = w.to_bytes().unwrap();
    assert_eq!(lookup::<u32>(&bytes, "8.8.8.8"), Some(3));
}

#[test]
fn metadata_pointer_option_changes_the_bytes() {
    // Identical databases except for the metadata-pointer option. The metadata contains a
    // dedupable repeated string ("en" appears as both a language and a description key), so
    // pointers-enabled output must be smaller than (and different from) pointers-disabled.
    let build = |pointers: MetadataPointers| {
        let mut w = Writer::builder("Ptr-Diff")
            .languages(["en"])
            .description(&[("en", "database")])
            .metadata_pointers(pointers)
            .build_epoch(std::time::SystemTime::UNIX_EPOCH)
            .build();
        w.insert_value(net("10.0.0.0/24"), Value::from(1_u32))
            .unwrap();
        w.to_bytes().unwrap()
    };
    let with_pointers = build(MetadataPointers::Enabled);
    let without_pointers = build(MetadataPointers::Disabled);
    assert!(
        without_pointers.len() > with_pointers.len(),
        "pointer-free metadata must be strictly larger ({} vs {})",
        without_pointers.len(),
        with_pointers.len()
    );
    // Both are valid databases with identical metadata content.
    for bytes in [&with_pointers, &without_pointers] {
        let r = reader(bytes);
        assert_eq!(r.metadata().database_type, "Ptr-Diff");
        assert_eq!(r.metadata().description.get("en").unwrap(), "database");
    }
}

#[test]
fn metadata_without_pointers_is_readable() {
    let mut w = Writer::builder("Meta-No-Pointers")
        .metadata_pointers(MetadataPointers::Disabled)
        .languages(["en", "de", "fr"])
        .build();
    w.insert_value(net("192.0.2.0/24"), Value::from(1_u32))
        .unwrap();
    let bytes = w.to_bytes().unwrap();

    let reader = reader(&bytes);
    assert_eq!(reader.metadata().database_type, "Meta-No-Pointers");
    assert_eq!(reader.metadata().languages.len(), 3);
    assert_eq!(lookup::<u32>(&bytes, "192.0.2.1"), Some(1));
}
