//! IP-version handling and the conversion from [`ipnet::IpNet`] to the `(bits, prefix_len)`
//! pair the tree walks.

// Narrowing casts are intentional: addresses and prefix lengths are range-bounded by the IP
// family before the cast (a v4 base is < 2^32, a prefix length <= 128).
#![allow(clippy::cast_possible_truncation)]

use std::net::IpAddr;

use ipnet::{IpNet, Ipv4Net, Ipv6Net};

use crate::error::Error;

/// Which IP version a database indexes.
///
/// A [`Writer`](crate::Writer) defaults to [`IpVersion::V6`], which stores IPv4 networks
/// inside the IPv4-in-IPv6 range (`::/96`) so a single database answers both IPv4 and IPv6
/// lookups. Choose [`IpVersion::V4`] for a smaller, IPv4-only database that rejects IPv6
/// inserts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum IpVersion {
    /// A 32-bit tree. Only IPv4 networks may be inserted.
    V4,
    /// A 128-bit tree. IPv4 networks are stored under `::/96`. **Default.**
    #[default]
    V6,
}

impl IpVersion {
    /// Depth of the search tree in bits (32 for IPv4, 128 for IPv6).
    pub(crate) const fn tree_depth(self) -> u8 {
        match self {
            Self::V4 => 32,
            Self::V6 => 128,
        }
    }

    /// The value written to the metadata `ip_version` field (4 or 6).
    pub(crate) const fn metadata(self) -> u16 {
        match self {
            Self::V4 => 4,
            Self::V6 => 6,
        }
    }
}

/// Convert a network to the `(bits, prefix_len)` pair used to walk the tree.
///
/// Host bits are truncated first (matching the Go writer). In a V6 tree, IPv4 networks are
/// lifted into `::/96`. In a V4 tree, the address occupies the top 32 bits of the walk word
/// and IPv6 inputs are rejected.
pub(crate) fn to_tree_prefix(net: IpNet, ip_version: IpVersion) -> Result<(u128, u8), Error> {
    match (ip_version, net.trunc()) {
        (IpVersion::V6, IpNet::V6(v6)) => {
            let bits = u128::from_be_bytes(v6.addr().octets());
            Ok((bits, v6.prefix_len()))
        }
        (IpVersion::V6, IpNet::V4(v4)) => {
            // IPv4 lives under `::/96`: leave the top 96 bits zero.
            let v4_bits = u32::from_be_bytes(v4.addr().octets());
            Ok((u128::from(v4_bits), 96 + v4.prefix_len()))
        }
        (IpVersion::V4, IpNet::V4(v4)) => {
            // The address occupies the top 32 bits so `bits >> (127 - depth)` reads it
            // most-significant-bit first over depths 0..32.
            let v4_bits = u32::from_be_bytes(v4.addr().octets());
            Ok((u128::from(v4_bits) << 96, v4.prefix_len()))
        }
        (IpVersion::V4, IpNet::V6(v6)) => Err(Error::Ipv6InIpv4Tree(v6)),
    }
}

/// The three IPv6 networks that are aliased to the IPv4 subtree: IPv4-mapped, Teredo, and
/// 6to4. Used to reject inserts that would land in aliased space.
pub(crate) fn alias_networks() -> [IpNet; 3] {
    [
        "::ffff:0:0/96".parse().expect("valid alias CIDR"),
        "2001::/32".parse().expect("valid alias CIDR"),
        "2002::/16".parse().expect("valid alias CIDR"),
    ]
}

/// Decompose an inclusive address range `[start, end]` into the minimal list of CIDR
/// networks that exactly covers it.
///
/// Both endpoints must be the same IP family. The result is ordered from `start` upward.
pub(crate) fn range_to_networks(start: IpAddr, end: IpAddr) -> Result<Vec<IpNet>, Error> {
    match (start, end) {
        (IpAddr::V4(s), IpAddr::V4(e)) => {
            let (s, e) = (u32::from(s), u32::from(e));
            if s > e {
                return Err(Error::InvalidRange(
                    "start address is greater than end address",
                ));
            }
            Ok(range_to_cidrs(u128::from(s), u128::from(e), 32)
                .into_iter()
                .map(|(base, prefix)| {
                    // `base` fits in 32 bits and `prefix <= 32`, so both casts are exact.
                    let addr = std::net::Ipv4Addr::from(base as u32);
                    IpNet::V4(Ipv4Net::new(addr, prefix).expect("prefix within range"))
                })
                .collect())
        }
        (IpAddr::V6(s), IpAddr::V6(e)) => {
            let (s, e) = (u128::from(s), u128::from(e));
            if s > e {
                return Err(Error::InvalidRange(
                    "start address is greater than end address",
                ));
            }
            Ok(range_to_cidrs(s, e, 128)
                .into_iter()
                .map(|(base, prefix)| {
                    let addr = std::net::Ipv6Addr::from(base);
                    IpNet::V6(Ipv6Net::new(addr, prefix).expect("prefix within range"))
                })
                .collect())
        }
        _ => Err(Error::InvalidRange(
            "start and end are different IP families",
        )),
    }
}

/// Core integer range → CIDR decomposition, operating in a `total_bits`-wide address space
/// (32 for IPv4, 128 for IPv6). Returns `(base, prefix_len)` pairs.
fn range_to_cidrs(mut start: u128, end: u128, total_bits: u32) -> Vec<(u128, u8)> {
    // Whole-space shortcut avoids a `1 << total_bits` overflow below.
    if start == 0 && end == max_for_bits(total_bits) {
        return vec![(0, 0)];
    }
    let mut cidrs = Vec::new();
    loop {
        // Largest block whose size is limited by `start`'s alignment...
        let align_bits = if start == 0 {
            total_bits
        } else {
            start.trailing_zeros().min(total_bits)
        };
        // ...and shrunk until the block fits within `end`.
        let mut size = align_bits;
        while size > 0 {
            let block = 1u128 << size;
            let block_end = start + (block - 1);
            if block_end <= end {
                break;
            }
            size -= 1;
        }
        let prefix_len = (total_bits - size) as u8;
        cidrs.push((start, prefix_len));

        let block = 1u128 << size;
        match start.checked_add(block) {
            Some(next) if next <= end => start = next,
            _ => break,
        }
    }
    cidrs
}

fn max_for_bits(total_bits: u32) -> u128 {
    if total_bits >= 128 {
        u128::MAX
    } else {
        (1u128 << total_bits) - 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v4(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn aligned_range_is_single_cidr() {
        let nets = range_to_networks(v4("10.0.0.0"), v4("10.0.0.255")).unwrap();
        assert_eq!(nets, vec!["10.0.0.0/24".parse().unwrap()]);
    }

    #[test]
    fn unaligned_range_splits() {
        // 10.0.0.1 - 10.0.0.2 → two /32s.
        let nets = range_to_networks(v4("10.0.0.1"), v4("10.0.0.2")).unwrap();
        assert_eq!(
            nets,
            vec![
                "10.0.0.1/32".parse().unwrap(),
                "10.0.0.2/32".parse().unwrap()
            ]
        );
    }

    #[test]
    fn classic_range_decomposition() {
        // 1.1.1.0 - 1.1.1.5 → /29? no: 0..5 = 0-3 (/30) + 4-5 (/31).
        let nets = range_to_networks(v4("1.1.1.0"), v4("1.1.1.5")).unwrap();
        assert_eq!(
            nets,
            vec!["1.1.1.0/30".parse().unwrap(), "1.1.1.4/31".parse().unwrap()]
        );
    }

    #[test]
    fn single_address_range() {
        let nets = range_to_networks(v4("192.168.1.1"), v4("192.168.1.1")).unwrap();
        assert_eq!(nets, vec!["192.168.1.1/32".parse().unwrap()]);
    }

    #[test]
    fn whole_ipv4_space() {
        let nets = range_to_networks(v4("0.0.0.0"), v4("255.255.255.255")).unwrap();
        assert_eq!(nets, vec!["0.0.0.0/0".parse().unwrap()]);
    }

    #[test]
    fn reversed_range_errors() {
        assert!(range_to_networks(v4("10.0.0.5"), v4("10.0.0.1")).is_err());
    }

    #[test]
    fn mismatched_families_error() {
        assert!(range_to_networks(v4("10.0.0.0"), "::1".parse().unwrap()).is_err());
    }

    #[test]
    fn ipv6_range() {
        let nets = range_to_networks(
            "2001:db8::".parse().unwrap(),
            "2001:db8::ff".parse().unwrap(),
        )
        .unwrap();
        assert_eq!(nets, vec!["2001:db8::/120".parse().unwrap()]);
    }

    #[test]
    fn ipv6_single_address_range() {
        let ip: IpAddr = "2001:db8::7".parse().unwrap();
        assert_eq!(
            range_to_networks(ip, ip).unwrap(),
            vec!["2001:db8::7/128".parse().unwrap()]
        );
    }

    #[test]
    fn ipv6_reversed_range_errors() {
        assert!(
            range_to_networks(
                "2001:db8::2".parse().unwrap(),
                "2001:db8::1".parse().unwrap()
            )
            .is_err()
        );
    }

    #[test]
    fn whole_ipv6_space() {
        let nets = range_to_networks(
            "::".parse().unwrap(),
            "ffff:ffff:ffff:ffff:ffff:ffff:ffff:ffff".parse().unwrap(),
        )
        .unwrap();
        assert_eq!(nets, vec!["::/0".parse().unwrap()]);
    }

    #[test]
    fn zero_start_partial_range_is_not_whole_space() {
        // Starts at 0.0.0.0 but does not reach the top: must NOT shortcut to /0.
        let nets = range_to_networks(v4("0.0.0.0"), v4("0.0.0.7")).unwrap();
        assert_eq!(nets, vec!["0.0.0.0/29".parse().unwrap()]);
        // Ends at the top but does not start at 0: also not /0.
        let nets = range_to_networks(v4("255.255.255.248"), v4("255.255.255.255")).unwrap();
        assert_eq!(nets, vec!["255.255.255.248/29".parse().unwrap()]);
    }
}
