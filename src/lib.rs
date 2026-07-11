#![doc = include_str!("../README.md")]

mod data_section;
mod error;
#[cfg(feature = "load")]
mod load;
mod metadata;
mod net;
mod options;
mod pool;
mod record_size;
mod reserved;
#[cfg(feature = "serde")]
mod ser;
mod tree;
mod value;
mod writer;

pub use crate::error::Error;
pub use crate::net::IpVersion;
pub use crate::options::{Ipv4Aliasing, MergeStrategy, MetadataPointers, ReservedNetworks};
pub use crate::record_size::RecordSize;
pub use crate::value::Value;
pub use crate::writer::{Writer, WriterBuilder};

/// The [`ipnet`] crate, re-exported for constructing the network types the insert methods
/// accept.
pub use ipnet;

/// A convenient result type alias for fallible operations in this crate.
pub type Result<T, E = Error> = std::result::Result<T, E>;
