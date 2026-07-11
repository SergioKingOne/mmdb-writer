//! Interning pool that maps [`Value`]s to small integer ids.
//!
//! The tree stores a [`ValueId`] at each populated leaf rather than an owned [`Value`], so
//! read-modify-write inserts ([`Writer::insert_with`]) can look the current value back up,
//! and identical values inserted at many networks are stored once. Encoding to the data
//! section happens later, at write time, over only the values still reachable from the tree.
//!
//! [`Writer::insert_with`]: crate::Writer::insert_with

use std::collections::HashMap;

use crate::value::Value;

/// A handle to an interned [`Value`]. Ids are dense (`0..len`) and stable for the pool's
/// lifetime.
pub(crate) type ValueId = u32;

/// Deduplicating store of [`Value`]s.
#[derive(Debug, Default)]
pub(crate) struct ValuePool {
    values: Vec<Value>,
    index: HashMap<Value, ValueId>,
}

impl ValuePool {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Intern `value`, returning its id. Equal values return the same id.
    ///
    /// # Panics
    ///
    /// Panics if more than `u32::MAX` distinct values are interned — far beyond any real
    /// database (each distinct value costs at least one data-section byte, so the pool would
    /// need gigabytes of unique payloads to reach the ceiling).
    pub(crate) fn intern(&mut self, value: Value) -> ValueId {
        if let Some(&id) = self.index.get(&value) {
            return id;
        }
        let id = ValueId::try_from(self.values.len()).expect("value pool exceeded u32::MAX ids");
        self.values.push(value.clone());
        self.index.insert(value, id);
        id
    }

    /// Look up a previously interned value by id.
    pub(crate) fn get(&self, id: ValueId) -> &Value {
        &self.values[id as usize]
    }
}
