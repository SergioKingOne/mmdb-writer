//! Tree record width — the number of bits used to encode each child pointer in a
//! binary-search-tree node.

/// Width, in bits, of a single child record inside a search-tree node.
///
/// Each node holds two records (a left and a right child). Wider records address larger
/// trees at the cost of bytes on disk. The [MMDB format] defines exactly these three widths;
/// any other width produces a file no reader will accept, so it is modeled as an enum rather
/// than a raw integer.
///
/// A [`Writer`] picks the smallest size that fits by default — see
/// [`WriterBuilder::record_size`] to pin one explicitly.
///
/// [MMDB format]: https://maxmind.github.io/MaxMind-DB/
/// [`Writer`]: crate::Writer
/// [`WriterBuilder::record_size`]: crate::WriterBuilder::record_size
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RecordSize {
    /// 24-bit records, 6-byte nodes. Capacity: ~16.7 million nodes.
    Bits24,
    /// 28-bit records, 7-byte nodes. Capacity: ~268 million nodes. **Default.**
    #[default]
    Bits28,
    /// 32-bit records, 8-byte nodes. Capacity: ~4.3 billion nodes.
    Bits32,
}

impl RecordSize {
    /// Width of the record in bits (24, 28, or 32).
    #[must_use]
    pub const fn bits(self) -> u8 {
        match self {
            Self::Bits24 => 24,
            Self::Bits28 => 28,
            Self::Bits32 => 32,
        }
    }

    /// Number of bytes a single tree node (two records) occupies on disk.
    #[must_use]
    pub const fn node_bytes(self) -> usize {
        match self {
            Self::Bits24 => 6,
            Self::Bits28 => 7,
            Self::Bits32 => 8,
        }
    }

    /// Maximum addressable record value, exclusive. Values at or above this limit cannot be
    /// encoded and are caught before serialization.
    #[must_use]
    pub const fn max_value(self) -> u64 {
        match self {
            Self::Bits24 => 1 << 24,
            Self::Bits28 => 1 << 28,
            Self::Bits32 => 1 << 32,
        }
    }

    /// Width encoded as a `u16` for the metadata section's `record_size` field.
    #[must_use]
    pub const fn as_metadata(self) -> u16 {
        self.bits() as u16
    }

    /// The record sizes in ascending order of capacity, for auto-selection.
    pub(crate) const ASCENDING: [Self; 3] = [Self::Bits24, Self::Bits28, Self::Bits32];
}
