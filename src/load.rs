//! Loading an existing `.mmdb` file into a [`Writer`] so it can be extended and rewritten.
//! Gated on the `load` feature (which pulls in the `maxminddb` reader).

use std::path::Path;
use std::time::{Duration, UNIX_EPOCH};

use ipnet::IpNet;
use maxminddb::{Reader, WithinOptions};

use crate::error::Error;
use crate::net::IpVersion;
use crate::record_size::RecordSize;
use crate::value::Value;
use crate::writer::Writer;

impl Writer {
    /// Load an existing database from a file, ready to be extended.
    ///
    /// The returned [`Writer`] is seeded with every network in the file and the file's
    /// metadata (database type, languages, descriptions, IP version, record size, and build
    /// epoch). Reserved-network and aliasing options are not stored in the format, so they
    /// take their defaults.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Io`] if the file cannot be read, or [`Error::Load`] if it is not a
    /// valid database.
    pub fn load_from_path(path: impl AsRef<Path>) -> Result<Self, Error> {
        let data = std::fs::read(path)?;
        Self::load(&data)
    }

    /// Load an existing database from a byte buffer, ready to be extended.
    ///
    /// See [`load_from_path`](Self::load_from_path) for details.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Load`] if `bytes` is not a valid database.
    pub fn load(bytes: impl AsRef<[u8]>) -> Result<Self, Error> {
        let reader = Reader::from_source(bytes.as_ref()).map_err(load_err)?;
        let meta = reader.metadata();

        let ip_version = if meta.ip_version == 4 {
            IpVersion::V4
        } else {
            IpVersion::V6
        };
        let epoch = UNIX_EPOCH + Duration::from_secs(meta.build_epoch);

        let mut writer = Writer::builder(meta.database_type.clone())
            .ip_version(ip_version)
            .languages(meta.languages.clone())
            .description(meta.description.clone())
            .record_size(record_size_from(meta.record_size)?)
            .build_epoch(epoch)
            .build();

        // Default options skip aliased networks (so IPv4 data is not yielded several times)
        // and networks without data.
        let items = reader
            .networks(WithinOptions::default())
            .map_err(load_err)?;
        for item in items {
            let item = item.map_err(load_err)?;
            let Some(value) = item.decode::<Value>().map_err(load_err)? else {
                continue;
            };
            let network = item.network().map_err(load_err)?;
            let net = IpNet::new(network.ip(), network.prefix())
                .map_err(|e| Error::Load(format!("reader produced an invalid network: {e}")))?;
            writer.insert_value(net, value)?;
        }

        Ok(writer)
    }
}

// Taken by value so it can be used directly as a `map_err` argument.
#[allow(clippy::needless_pass_by_value)]
fn load_err(e: maxminddb::MaxMindDbError) -> Error {
    Error::Load(e.to_string())
}

fn record_size_from(bits: u16) -> Result<RecordSize, Error> {
    match bits {
        24 => Ok(RecordSize::Bits24),
        28 => Ok(RecordSize::Bits28),
        32 => Ok(RecordSize::Bits32),
        other => Err(Error::Load(format!("unsupported record size {other}"))),
    }
}
