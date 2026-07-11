//! The crate's error type.

use crate::record_size::RecordSize;

/// Errors returned while building or writing a database.
///
/// This type implements [`std::error::Error`] and is `Send + Sync + 'static`, so it composes
/// with [`Box<dyn Error>`](std::error::Error) and error libraries such as `anyhow`.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// A value could not be represented in MMDB's type system.
    ///
    /// The most common cause is an `i64`/`isize` field (MMDB has no signed 64-bit type), a
    /// non-string map key, or a top-level `Option::None`.
    #[error("value cannot be represented in MMDB: {0}")]
    UnsupportedValue(&'static str),

    /// A `serde::Serialize` value failed to serialize.
    #[cfg(feature = "serde")]
    #[error("serialization failed: {0}")]
    Serialize(String),

    /// An IPv6 network was inserted into an IPv4-only (`ip_version = 4`) database.
    #[error("cannot insert IPv6 network {0} into an IPv4 database")]
    Ipv6InIpv4Tree(ipnet::Ipv6Net),

    /// A range passed to [`Writer::insert_range`](crate::Writer::insert_range) was invalid —
    /// the endpoints are different IP families, or the start is above the end.
    #[error("invalid IP range: {0}")]
    InvalidRange(&'static str),

    /// An insert targeted a network reserved for IPv4 aliasing
    /// (`::ffff:0:0/96`, `2001::/32`, or `2002::/16`).
    #[error("cannot insert into aliased network {0}")]
    AliasedNetwork(ipnet::IpNet),

    /// An insert targeted a reserved network while reserved networks were excluded (the
    /// default). Enable [`ReservedNetworks::Included`] to write into reserved space.
    ///
    /// [`ReservedNetworks::Included`]: crate::ReservedNetworks::Included
    #[error("cannot insert into reserved network {0}")]
    ReservedNetwork(ipnet::IpNet),

    /// The tree (plus data section) grew past what the chosen [`RecordSize`] can address.
    ///
    /// When the record size is chosen automatically this cannot happen below the 32-bit
    /// ceiling; it only surfaces when a smaller size was pinned explicitly, or for a
    /// genuinely enormous database.
    #[error("tree has {node_count} nodes but {record_size:?} can address at most {max}")]
    TreeTooLarge {
        /// Number of nodes in the (compacted) tree.
        node_count: usize,
        /// Maximum record value the chosen size can encode.
        max: u64,
        /// The record size that was too small.
        record_size: RecordSize,
    },

    /// Writing to the destination [`std::io::Write`] failed.
    #[error(transparent)]
    Io(#[from] std::io::Error),

    /// Reading an existing database with [`Writer::load`](crate::Writer::load) failed.
    #[cfg(feature = "load")]
    #[error("failed to load database: {0}")]
    Load(String),
}
