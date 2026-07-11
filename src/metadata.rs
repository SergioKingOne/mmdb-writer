//! Encode the metadata section — the map of fields that follows the
//! `\xab\xcd\xefMaxMind.com` marker and tells a reader how to interpret the file.

use std::collections::BTreeMap;

use crate::data_section::DataSection;
use crate::net::IpVersion;
use crate::record_size::RecordSize;
use crate::value::Value;

const BINARY_FORMAT_MAJOR_VERSION: u16 = 2;
const BINARY_FORMAT_MINOR_VERSION: u16 = 0;

/// The fields written to the metadata section. Borrows from the [`Writer`](crate::Writer) so
/// no ownership is taken.
pub(crate) struct Metadata<'a> {
    pub database_type: &'a str,
    pub description: &'a BTreeMap<String, String>,
    pub languages: &'a [String],
    pub ip_version: IpVersion,
    pub record_size: RecordSize,
    pub node_count: usize,
    pub build_epoch: u64,
    pub disable_pointers: bool,
}

impl Metadata<'_> {
    /// Encode the metadata map to its own self-contained byte sequence.
    ///
    /// A fresh [`DataSection`] is used so any pointers it emits refer only to offsets within
    /// the metadata itself, independent of the main data section.
    pub(crate) fn to_bytes(&self) -> Vec<u8> {
        let mut map = BTreeMap::new();
        map.insert(
            "binary_format_major_version".to_owned(),
            Value::U16(BINARY_FORMAT_MAJOR_VERSION),
        );
        map.insert(
            "binary_format_minor_version".to_owned(),
            Value::U16(BINARY_FORMAT_MINOR_VERSION),
        );
        map.insert("build_epoch".to_owned(), Value::U64(self.build_epoch));
        map.insert(
            "database_type".to_owned(),
            Value::String(self.database_type.to_owned()),
        );
        let description = self
            .description
            .iter()
            .map(|(lang, text)| (lang.clone(), Value::String(text.clone())))
            .collect::<BTreeMap<_, _>>();
        map.insert("description".to_owned(), Value::Map(description));
        map.insert(
            "ip_version".to_owned(),
            Value::U16(self.ip_version.metadata()),
        );
        let languages = self
            .languages
            .iter()
            .map(|s| Value::String(s.clone()))
            .collect();
        map.insert("languages".to_owned(), Value::Array(languages));
        map.insert(
            "node_count".to_owned(),
            Value::U32(u32::try_from(self.node_count).unwrap_or(u32::MAX)),
        );
        map.insert(
            "record_size".to_owned(),
            Value::U16(self.record_size.as_metadata()),
        );

        let mut data = if self.disable_pointers {
            DataSection::without_pointers()
        } else {
            DataSection::new()
        };
        data.push(&Value::Map(map));
        data.into_bytes()
    }
}
