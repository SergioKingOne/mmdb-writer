//! Shared helpers for the integration tests: build a database with the writer, then read it
//! back through the official `maxminddb` reader as an independent oracle.
#![allow(dead_code, unreachable_pub)]

use std::net::IpAddr;

use maxminddb::Reader;
use serde::de::DeserializeOwned;

/// Parse an MMDB byte buffer with the reference reader.
pub fn reader(bytes: &[u8]) -> Reader<&[u8]> {
    Reader::from_source(bytes).expect("writer produced a readable database")
}

/// Look up `ip` in `bytes` and decode the record into `T`, returning `None` on a miss.
pub fn lookup<T: DeserializeOwned>(bytes: &[u8], ip: &str) -> Option<T> {
    let addr: IpAddr = ip.parse().expect("valid IP");
    reader(bytes)
        .lookup(addr)
        .expect("lookup did not error")
        .decode::<T>()
        .expect("record decodes into T")
}
