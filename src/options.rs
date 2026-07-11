//! Behavioral option enums for [`Writer`](crate::Writer), modeled as enums rather than bare
//! booleans so call sites read clearly and future variants stay non-breaking.

/// Whether to install IPv4 aliases in an [`IpVersion::V6`](crate::IpVersion::V6) database.
///
/// When enabled (the default), queries arriving in IPv4-mapped (`::ffff:0:0/96`), 6to4
/// (`2002::/16`), and Teredo (`2001::/32`) form resolve to the same data as the plain IPv4
/// lookup. This has no effect on IPv4-only databases.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum Ipv4Aliasing {
    /// Install the IPv4 alias subtrees. **Default.**
    #[default]
    Enabled,
    /// Do not install aliases; IPv4-mapped/6to4/Teredo queries will not resolve to IPv4 data.
    Disabled,
}

impl Ipv4Aliasing {
    pub(crate) const fn is_enabled(self) -> bool {
        matches!(self, Self::Enabled)
    }
}

/// Whether reserved (private, documentation, multicast, …) networks are writable.
///
/// # Default differs from the Go writer
///
/// This crate defaults to [`ReservedNetworks::Included`] — inserts into reserved space are
/// allowed — whereas the Go `mmdbwriter` excludes them by default. The permissive default is
/// deliberate: documentation ranges such as `192.0.2.0/24` and `2001:db8::/32` are reserved,
/// and rejecting them would make the most common example and test networks fail. Choose
/// [`ReservedNetworks::Excluded`] to match the Go behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum ReservedNetworks {
    /// Allow inserting into reserved networks. **Default** (differs from the Go writer).
    #[default]
    Included,
    /// Reject inserts that target reserved space (returning [`Error::ReservedNetwork`]) and
    /// carve reserved networks out of any broader insert that covers them.
    ///
    /// [`Error::ReservedNetwork`]: crate::Error::ReservedNetwork
    Excluded,
}

impl ReservedNetworks {
    pub(crate) const fn is_excluded(self) -> bool {
        matches!(self, Self::Excluded)
    }
}

/// Whether the metadata section may use data-section pointers.
///
/// Pointers make the metadata marginally smaller by sharing repeated strings, but a few
/// historic readers mishandle them. Disable to emit fully inline metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum MetadataPointers {
    /// Allow pointers in the metadata section. **Default.**
    #[default]
    Enabled,
    /// Emit metadata without any pointers.
    Disabled,
}

impl MetadataPointers {
    pub(crate) const fn is_disabled(self) -> bool {
        matches!(self, Self::Disabled)
    }
}

/// How [`Writer::insert_merged`](crate::Writer::insert_merged) combines a new value with the
/// value already covering a network.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum MergeStrategy {
    /// Replace the existing value entirely (identical to
    /// [`insert`](crate::Writer::insert)). **Default.**
    #[default]
    Replace,
    /// Merge the top level of two maps: the union of keys, with the new value winning on
    /// conflicts. See [`Value::merge_top_level`](crate::Value::merge_top_level).
    TopLevelMerge,
    /// Recursively merge nested maps and concatenate arrays. See
    /// [`Value::merge_deep`](crate::Value::merge_deep).
    DeepMerge,
}
