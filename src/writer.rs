//! [`Writer`] — the public entry point. Accumulates `(network, value)` inserts and produces
//! the final `.mmdb` byte sequence.
//!
//! ## File layout ([MMDB spec] §2)
//!
//! ```text
//!   [ search-tree bytes ]
//!   [ 16 × 0x00        data-section separator ]
//!   [ data-section bytes ]
//!   [ \xab\xcd\xef MaxMind.com   metadata marker (14 bytes) ]
//!   [ metadata map bytes ]
//! ```
//!
//! A reader scans backward from the end for the last occurrence of the metadata marker, then
//! decodes the map that follows.
//!
//! [MMDB spec]: https://maxmind.github.io/MaxMind-DB/

use std::collections::{BTreeMap, HashMap};
use std::io;
use std::net::IpAddr;
use std::time::{SystemTime, UNIX_EPOCH};

use ipnet::IpNet;

use crate::data_section::{DataOffset, DataSection};
use crate::error::Error;
use crate::metadata::Metadata;
use crate::net::{IpVersion, alias_networks, range_to_networks, to_tree_prefix};
use crate::options::{Ipv4Aliasing, MergeStrategy, MetadataPointers, ReservedNetworks};
use crate::pool::{ValueId, ValuePool};
use crate::record_size::RecordSize;
use crate::reserved;
use crate::tree::Tree;
use crate::value::Value;

/// 16 bytes of `0x00` separating the tree section from the data section.
const DATA_SECTION_SEPARATOR: [u8; 16] = [0; 16];

/// Marker that precedes the metadata section. Readers scan for its last occurrence.
const METADATA_MARKER: &[u8; 14] = b"\xab\xcd\xefMaxMind.com";

fn default_languages() -> Vec<String> {
    vec!["en".to_owned()]
}

/// Builds a MaxMind DB and serializes it.
///
/// Construct one with [`Writer::new`] for the common case, or [`Writer::builder`] to set
/// options such as the [`IpVersion`], descriptions, or a fixed [`RecordSize`]. Then add data
/// with [`insert`](Writer::insert) / [`insert_value`](Writer::insert_value) and produce bytes
/// with [`to_bytes`](Writer::to_bytes) or [`write_to`](Writer::write_to).
///
/// ```
/// use ipnet::IpNet;
/// use mmdb_writer::{Value, Writer};
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let mut writer = Writer::new("Example-DB");
/// writer.insert_value(
///     "192.0.2.0/24".parse::<IpNet>()?,
///     Value::map([("hello", Value::from("world"))]),
/// )?;
/// let bytes = writer.to_bytes()?;
/// assert!(!bytes.is_empty());
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct Writer {
    database_type: String,
    description: BTreeMap<String, String>,
    languages: Vec<String>,
    ip_version: IpVersion,
    record_size: Option<RecordSize>,
    ipv4_aliasing: Ipv4Aliasing,
    reserved_networks: ReservedNetworks,
    metadata_pointers: MetadataPointers,
    build_epoch: Option<SystemTime>,
    tree: Tree,
    pool: ValuePool,
}

#[bon::bon]
impl Writer {
    /// Start building a [`Writer`] with options.
    ///
    /// `database_type` names the database for readers (conventionally `Vendor-Dataset`).
    /// Every other option has a default, so the terminal [`build`](WriterBuilder::build) can
    /// follow immediately.
    ///
    /// ```
    /// use mmdb_writer::{IpVersion, RecordSize, Writer};
    ///
    /// let writer = Writer::builder("Example-DB")
    ///     .ip_version(IpVersion::V4)
    ///     .record_size(RecordSize::Bits32)
    ///     .languages(["en", "de"])
    ///     .build();
    /// ```
    #[builder(builder_type = WriterBuilder, finish_fn = build)]
    pub fn builder(
        #[builder(start_fn, into)] database_type: String,
        /// Locales the descriptions cover. Defaults to `["en"]`.
        #[builder(default = default_languages(), with = |langs: impl IntoIterator<Item: Into<String>>| langs.into_iter().map(Into::into).collect())]
        languages: Vec<String>,
        /// Per-language description strings (language code → text). Defaults to empty.
        #[builder(default)]
        description: BTreeMap<String, String>,
        /// IP version of the database. Defaults to [`IpVersion::V6`].
        #[builder(default)]
        ip_version: IpVersion,
        /// Fixed tree record size. Defaults to automatic selection (smallest that fits).
        record_size: Option<RecordSize>,
        /// Whether to install IPv4 aliases (V6 only). Defaults to
        /// [`Ipv4Aliasing::Enabled`].
        #[builder(default)]
        ipv4_aliasing: Ipv4Aliasing,
        /// Whether reserved networks are writable. Defaults to
        /// [`ReservedNetworks::Included`] (note: this differs from the Go writer).
        #[builder(default)]
        reserved_networks: ReservedNetworks,
        /// Whether the metadata section may use pointers. Defaults to
        /// [`MetadataPointers::Enabled`].
        #[builder(default)]
        metadata_pointers: MetadataPointers,
        /// Build timestamp written to the metadata. Defaults to the current time; set it for
        /// reproducible output.
        build_epoch: Option<SystemTime>,
    ) -> Self {
        let mut tree = Tree::new();
        if reserved_networks.is_excluded() {
            for network in reserved::networks(ip_version) {
                let (bits, prefix_len) =
                    to_tree_prefix(network, ip_version).expect("reserved network fits the tree");
                tree.paint_reserved(bits, prefix_len)
                    .expect("reserved network fits the tree");
            }
        }
        Self {
            database_type,
            description,
            languages,
            ip_version,
            record_size,
            ipv4_aliasing,
            reserved_networks,
            metadata_pointers,
            build_epoch,
            tree,
            pool: ValuePool::new(),
        }
    }
}

impl Writer {
    /// Create a [`Writer`] with all default options and the given database type.
    #[must_use]
    pub fn new(database_type: impl Into<String>) -> Self {
        Self::builder(database_type).build()
    }

    /// Insert any [`Serialize`](serde::Serialize) value at every address in `network`.
    ///
    /// The value is projected onto the MMDB type system: structs and maps become MMDB maps,
    /// sequences become arrays, `Option::None` and unit values are dropped, and enums are
    /// serialized in serde's externally tagged form. Later inserts win, as with
    /// [`insert_value`](Self::insert_value).
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnsupportedValue`] if the value uses a type MMDB cannot represent
    /// (such as `i64`), [`Error::Serialize`] if serialization fails, or
    /// [`Error::Ipv6InIpv4Tree`] for an IPv6 network in an IPv4 database.
    ///
    /// ```
    /// use ipnet::IpNet;
    /// use mmdb_writer::Writer;
    /// use serde::Serialize;
    ///
    /// #[derive(Serialize)]
    /// struct Asn { autonomous_system_number: u32, autonomous_system_organization: String }
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let mut writer = Writer::new("ASN-DB");
    /// writer.insert(
    ///     "192.0.2.0/24".parse::<IpNet>()?,
    ///     &Asn { autonomous_system_number: 64_512, autonomous_system_organization: "Example".into() },
    /// )?;
    /// # Ok(())
    /// # }
    /// ```
    #[cfg(feature = "serde")]
    pub fn insert<N: Into<IpNet>, T: serde::Serialize + ?Sized>(
        &mut self,
        network: N,
        value: &T,
    ) -> Result<(), Error> {
        let value = crate::ser::to_value(value)?;
        self.insert_value(network, value)
    }

    /// Insert a [`Value`] at every address in `network`.
    ///
    /// Later inserts win: a more-specific network inserted afterward overrides the covered
    /// addresses, and a less-specific one overwrites everything it covers. Host bits in
    /// `network` are ignored.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Ipv6InIpv4Tree`] if `network` is IPv6 but the database is
    /// [`IpVersion::V4`].
    pub fn insert_value<N: Into<IpNet>>(&mut self, network: N, value: Value) -> Result<(), Error> {
        let net = network.into();
        self.ensure_insertable(net)?;
        let (bits, prefix_len) = to_tree_prefix(net, self.ip_version)?;
        let id = self.pool.intern(value);
        self.tree.insert(bits, prefix_len, &mut |_| Some(id))?;
        Ok(())
    }

    /// Reject inserts that target aliased or (when excluded) reserved space, matching the Go
    /// writer's `AliasedNetworkError` / `ReservedNetworkError`.
    fn ensure_insertable(&self, net: IpNet) -> Result<(), Error> {
        use ipnet::IpNet as N;
        // `contains` is true when `net` is equal to or inside the blocking network. A `net`
        // that *contains* a blocked network is allowed — it is carved out at build time.
        let contains = |outer: &N, inner: &N| outer.contains(inner);
        if self.ip_version == IpVersion::V6
            && self.ipv4_aliasing.is_enabled()
            && alias_networks().iter().any(|a| contains(a, &net))
        {
            return Err(Error::AliasedNetwork(net));
        }
        if self.reserved_networks.is_excluded()
            && reserved::networks(self.ip_version)
                .iter()
                .any(|r| contains(r, &net))
        {
            return Err(Error::ReservedNetwork(net));
        }
        Ok(())
    }

    /// Insert into `network` by computing each covered leaf's new value from its current one.
    ///
    /// The operation receives the value currently covering a leaf (`None` where there is
    /// none) and returns the value to store, or `None` to clear it. Because a network can
    /// cover many existing leaves, the operation may be called more than once per insert —
    /// once per distinct value it paints over. This mirrors the Go writer's `InsertFunc`.
    ///
    /// ```
    /// use ipnet::IpNet;
    /// use mmdb_writer::{Value, Writer};
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let mut w = Writer::new("Counter");
    /// let mut bump = |net: &str| {
    ///     w.insert_with(net.parse::<IpNet>().unwrap(), |existing| {
    ///         let n = match existing {
    ///             Some(Value::U32(n)) => *n + 1,
    ///             _ => 1,
    ///         };
    ///         Some(Value::from(n))
    ///     })
    ///     .unwrap();
    /// };
    /// bump("192.0.2.0/24");
    /// bump("192.0.2.0/25"); // overlaps: sees the existing 1, stores 2
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// Returns [`Error::Ipv6InIpv4Tree`] for an IPv6 network in an IPv4 database.
    pub fn insert_with<N, F>(&mut self, network: N, mut op: F) -> Result<(), Error>
    where
        N: Into<IpNet>,
        F: FnMut(Option<&Value>) -> Option<Value>,
    {
        let net = network.into();
        self.ensure_insertable(net)?;
        let (bits, prefix_len) = to_tree_prefix(net, self.ip_version)?;
        let tree = &mut self.tree;
        let pool = &mut self.pool;
        tree.insert(bits, prefix_len, &mut |old_id| {
            let old_value = old_id.map(|id| pool.get(id).clone());
            op(old_value.as_ref()).map(|new_value| pool.intern(new_value))
        })
    }

    /// Insert a [`Value`] into `network`, combining it with any existing value per `strategy`.
    ///
    /// With [`MergeStrategy::Replace`] this is [`insert_value`](Self::insert_value); the other
    /// strategies merge maps (and, for [`DeepMerge`](MergeStrategy::DeepMerge), nested maps
    /// and arrays) rather than overwriting.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Ipv6InIpv4Tree`] for an IPv6 network in an IPv4 database.
    pub fn insert_value_merged<N: Into<IpNet>>(
        &mut self,
        network: N,
        value: Value,
        strategy: MergeStrategy,
    ) -> Result<(), Error> {
        match strategy {
            MergeStrategy::Replace => self.insert_value(network, value),
            MergeStrategy::TopLevelMerge => self.insert_with(network, |existing| {
                Some(match existing {
                    Some(old) => Value::merge_top_level(old, &value),
                    None => value.clone(),
                })
            }),
            MergeStrategy::DeepMerge => self.insert_with(network, |existing| {
                Some(match existing {
                    Some(old) => Value::merge_deep(old, &value),
                    None => value.clone(),
                })
            }),
        }
    }

    /// Insert any [`Serialize`](serde::Serialize) value into `network`, merging per `strategy`.
    ///
    /// The serde equivalent of [`insert_value_merged`](Self::insert_value_merged).
    ///
    /// # Errors
    ///
    /// As [`insert`](Self::insert) plus [`insert_value_merged`](Self::insert_value_merged).
    #[cfg(feature = "serde")]
    pub fn insert_merged<N: Into<IpNet>, T: serde::Serialize + ?Sized>(
        &mut self,
        network: N,
        value: &T,
        strategy: MergeStrategy,
    ) -> Result<(), Error> {
        let value = crate::ser::to_value(value)?;
        self.insert_value_merged(network, value, strategy)
    }

    /// Insert a [`Value`] at every address in the inclusive range `[start, end]`.
    ///
    /// The range is decomposed into the minimal set of CIDR networks and each is inserted
    /// (with [`insert_value`](Self::insert_value) semantics). Useful for data sources that
    /// express coverage as start–end pairs rather than CIDRs.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidRange`] if `start` and `end` are different IP families or
    /// `start` is above `end`, or [`Error::Ipv6InIpv4Tree`] for an IPv6 range in an IPv4
    /// database.
    pub fn insert_range(&mut self, start: IpAddr, end: IpAddr, value: &Value) -> Result<(), Error> {
        for network in range_to_networks(start, end)? {
            self.insert_value(network, value.clone())?;
        }
        Ok(())
    }

    /// Look up the value currently covering `ip`, if any.
    ///
    /// Reflects the most-specific matching insert, including the effect of merges and
    /// removals. Handy for tests and debugging. IPv4 aliases are not consulted (they are
    /// installed only at serialization time), so query IPv4 addresses directly.
    #[must_use]
    pub fn get(&self, ip: IpAddr) -> Option<&Value> {
        let (bits, _) = to_tree_prefix(IpNet::from(ip), self.ip_version).ok()?;
        let id = self.tree.get(bits, self.ip_version.tree_depth())?;
        Some(self.pool.get(id))
    }

    /// Serialize the database to a byte vector.
    ///
    /// This does not consume the writer, so more data can be inserted afterward and the
    /// database re-serialized.
    ///
    /// # Errors
    ///
    /// Returns [`Error::TreeTooLarge`] if the tree and data section exceed what the chosen
    /// [`RecordSize`] can address.
    pub fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        self.build()
    }

    /// Serialize the database, writing it to `writer`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::TreeTooLarge`] as [`to_bytes`](Self::to_bytes) does, or
    /// [`Error::Io`] if writing fails.
    pub fn write_to<W: io::Write>(&self, mut writer: W) -> Result<(), Error> {
        let bytes = self.build()?;
        writer.write_all(&bytes)?;
        Ok(())
    }

    fn build(&self) -> Result<Vec<u8>, Error> {
        // Compaction and aliasing mutate the tree; work on a copy so `self` is untouched.
        let mut tree = self.tree.clone();
        if self.ip_version == IpVersion::V6 && self.ipv4_aliasing.is_enabled() {
            tree.install_ipv4_aliases()?;
        }
        tree.compact();

        // Encode only the values still reachable from the tree.
        let ids = tree.reachable_data_ids();
        let mut data = DataSection::new();
        let mut id_to_offset: HashMap<ValueId, DataOffset> = HashMap::with_capacity(ids.len());
        for id in ids {
            let offset = data.push(self.pool.get(id));
            id_to_offset.insert(id, offset);
        }
        let data_len = data.len();
        let data_bytes = data.into_bytes();

        let record_size = pick_record_size(&tree, data_len, self.record_size)?;
        let tree_bytes = tree.serialize(record_size, &id_to_offset)?;

        let metadata = Metadata {
            database_type: &self.database_type,
            description: &self.description,
            languages: &self.languages,
            ip_version: self.ip_version,
            record_size,
            node_count: tree.node_count(),
            build_epoch: self.build_epoch_secs(),
            disable_pointers: self.metadata_pointers.is_disabled(),
        };
        let metadata_bytes = metadata.to_bytes();

        let mut out = Vec::with_capacity(
            tree_bytes.len()
                + DATA_SECTION_SEPARATOR.len()
                + data_bytes.len()
                + METADATA_MARKER.len()
                + metadata_bytes.len(),
        );
        out.extend_from_slice(&tree_bytes);
        out.extend_from_slice(&DATA_SECTION_SEPARATOR);
        out.extend_from_slice(&data_bytes);
        out.extend_from_slice(METADATA_MARKER);
        out.extend_from_slice(&metadata_bytes);
        Ok(out)
    }

    fn build_epoch_secs(&self) -> u64 {
        self.build_epoch
            .unwrap_or_else(SystemTime::now)
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs())
    }
}

/// Choose the smallest record size that fits the compacted tree plus data section, unless one
/// was pinned explicitly.
fn pick_record_size(
    tree: &Tree,
    data_len: usize,
    requested: Option<RecordSize>,
) -> Result<RecordSize, Error> {
    if let Some(size) = requested {
        return Ok(size);
    }
    for candidate in RecordSize::ASCENDING {
        if tree.fits_record_size(candidate, data_len) {
            return Ok(candidate);
        }
    }
    Err(Error::TreeTooLarge {
        node_count: tree.node_count(),
        max: RecordSize::Bits32.max_value(),
        record_size: RecordSize::Bits32,
    })
}
