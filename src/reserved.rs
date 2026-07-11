//! The reserved-network lists, matching the Go `mmdbwriter`'s `reserved.go`.
//!
//! These are only consulted when a [`Writer`](crate::Writer) is configured with
//! [`ReservedNetworks::Excluded`](crate::ReservedNetworks::Excluded). Each is a network that
//! should not carry public IP-intelligence data (private ranges, documentation ranges,
//! multicast, and so on).
//!
//! Both the IPv4 and IPv6 lists apply to an IPv6 database; only the IPv4 list applies to an
//! IPv4 database.

use ipnet::IpNet;

use crate::net::IpVersion;

/// IPv4 reserved networks (RFC 5735, RFC 6598, and friends).
const IPV4: &[&str] = &[
    "0.0.0.0/8",
    "10.0.0.0/8",
    "100.64.0.0/10",
    "127.0.0.0/8",
    "169.254.0.0/16",
    "172.16.0.0/12",
    "192.0.0.0/29",
    "192.0.2.0/24",
    "192.88.99.0/24",
    "192.168.0.0/16",
    "198.18.0.0/15",
    "198.51.100.0/24",
    "203.0.113.0/24",
    "224.0.0.0/4",
    "240.0.0.0/4",
];

/// IPv6 reserved networks.
const IPV6: &[&str] = &[
    "100::/64",
    "2001:1::/32",
    "2001:2::/31",
    "2001:4::/30",
    "2001:8::/29",
    "2001:10::/28",
    "2001:20::/27",
    "2001:40::/26",
    "2001:80::/25",
    "2001:100::/24",
    "2001:db8::/32",
    "fc00::/7",
    "fe80::/10",
    "ff00::/8",
];

/// The reserved networks that apply to a database of the given IP version.
///
/// An IPv6 database reserves both the IPv4 (via `::/96`) and IPv6 ranges; an IPv4 database
/// reserves only the IPv4 ranges.
pub(crate) fn networks(ip_version: IpVersion) -> Vec<IpNet> {
    let lists: &[&[&str]] = match ip_version {
        IpVersion::V4 => &[IPV4],
        IpVersion::V6 => &[IPV4, IPV6],
    };
    lists
        .iter()
        .flat_map(|list| list.iter())
        .map(|s| s.parse().expect("valid reserved CIDR"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_reserved_cidrs_parse() {
        assert_eq!(networks(IpVersion::V4).len(), IPV4.len());
        assert_eq!(networks(IpVersion::V6).len(), IPV4.len() + IPV6.len());
    }
}
