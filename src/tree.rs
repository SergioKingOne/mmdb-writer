// Bit-packing narrowing casts are intentional — tree records are encoded into fixed-width
// bytes per MMDB's record-size classes, and we range-check `raw < record_size.max_value()`
// before emitting.
#![allow(clippy::cast_possible_truncation)]

//! Binary-search tree (a bit-by-bit trie over the address space) that readers walk to
//! resolve an IP address to a data-section offset.
//!
//! # Algorithm — matches MaxMind's reference `mmdbwriter`
//!
//! Insertion walks the address bits from the most significant. Two regimes:
//!
//! 1. **Above the prefix** (`depth < prefix_len`): follow one bit into the chosen child.
//! 2. **At or below the prefix** (`depth >= prefix_len`): *broadcast* — recurse into both
//!    children so the operation is applied to every reachable leaf.
//!
//! Descending past an existing data leaf splits it into a node whose both children carry the
//! old value, then one side is recursed into — preserving the old value on the siblings the
//! new insert does not touch.
//!
//! Rather than a fixed value, each leaf visit applies a caller-supplied *operation*
//! (`FnMut(Option<ValueId>) -> Option<ValueId>`): given the id currently at the leaf (if
//! any) it returns the id to store, or `None` to clear it. Plain replacement is the constant
//! operation; read-modify-write and merges compute from the existing id.
//!
//! # IPv4 in IPv6 trees
//!
//! IPv4 networks are inserted under `::/96`. Before serialization,
//! [`Tree::install_ipv4_aliases`] pins that subtree root as a [`Record::FixedNode`] and adds
//! alias pointers so queries arriving in IPv4-mapped (`::ffff:0:0/96`), 6to4 (`2002::/16`),
//! and Teredo (`2001::/32`) form resolve to it.

use std::collections::{HashMap, HashSet};

use crate::data_section::DataOffset;
use crate::error::Error;
use crate::pool::ValueId;
use crate::record_size::RecordSize;

/// A single tree node: two child records.
#[derive(Debug, Clone, Copy, Default)]
struct Node {
    left: Record,
    right: Record,
}

impl Node {
    fn child(&self, bit: u8) -> Record {
        if bit == 0 { self.left } else { self.right }
    }

    fn child_mut(&mut self, bit: u8) -> &mut Record {
        if bit == 0 {
            &mut self.left
        } else {
            &mut self.right
        }
    }
}

/// A single record (one child of a node).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Record {
    /// No data; terminal. Encodes as `node_count`.
    #[default]
    NoData,
    /// Pointer to another node by index. Encodes as the node index (`< node_count`).
    Node(u32),
    /// Pointer to a node that must never be collapsed by [`Tree::compact`]. Used for the
    /// IPv4 subtree root at `::/96`, which a reader's `find_ipv4_start` walk expects to be a
    /// real node. Encodes identically to [`Record::Node`].
    FixedNode(u32),
    /// A populated leaf, referencing an interned value by id. Encodes as
    /// `node_count + 16 + data_offset`, where the offset comes from a value → offset map
    /// built at serialization time.
    Data(ValueId),
    /// Pointer to a shared subtree (an IPv4 alias). Encodes identically to [`Record::Node`]
    /// but is not recursed into during renumbering — the target is reached via its
    /// [`Record::FixedNode`] referent.
    Alias(u32),
    /// A reserved-network leaf: reachable but never holding data, so inserts broadcasting
    /// over it skip it (carving the reserved space out). Encodes as `node_count`, identical
    /// to [`Record::NoData`] on the wire.
    Reserved,
}

/// Bit-by-bit trie builder.
#[derive(Debug, Clone)]
pub(crate) struct Tree {
    nodes: Vec<Node>,
}

impl Tree {
    pub(crate) fn new() -> Self {
        // Always have a root node; callers expect `nodes[0]` to exist.
        Self {
            nodes: vec![Node::default()],
        }
    }

    pub(crate) fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Apply `op` to every leaf covered by the network `(bits, prefix_len)`.
    ///
    /// `op` receives the id currently at each covered leaf (`None` if empty) and returns the
    /// id to store there (`None` clears it). See the module docs for the walk.
    pub(crate) fn insert<F>(&mut self, bits: u128, prefix_len: u8, op: &mut F) -> Result<(), Error>
    where
        F: FnMut(Option<ValueId>) -> Option<ValueId>,
    {
        self.insert_at_node(0, 0, bits, prefix_len, op)
    }

    fn insert_at_node<F>(
        &mut self,
        node_idx: u32,
        current_depth: u8,
        bits: u128,
        prefix_len: u8,
        op: &mut F,
    ) -> Result<(), Error>
    where
        F: FnMut(Option<ValueId>) -> Option<ValueId>,
    {
        let new_depth = current_depth + 1;
        if new_depth > prefix_len {
            // Below the prefix: broadcast into both children.
            self.insert_at_record(node_idx, 0, new_depth, bits, prefix_len, op)?;
            self.insert_at_record(node_idx, 1, new_depth, bits, prefix_len, op)?;
        } else {
            let bit = ((bits >> (127 - current_depth)) & 1) as u8;
            self.insert_at_record(node_idx, bit, new_depth, bits, prefix_len, op)?;
        }
        Ok(())
    }

    fn insert_at_record<F>(
        &mut self,
        node_idx: u32,
        which_bit: u8,
        new_depth: u8,
        bits: u128,
        prefix_len: u8,
        op: &mut F,
    ) -> Result<(), Error>
    where
        F: FnMut(Option<ValueId>) -> Option<ValueId>,
    {
        let record = self.nodes[node_idx as usize].child(which_bit);
        match record {
            Record::Node(child_idx) => {
                self.insert_at_node(child_idx, new_depth, bits, prefix_len, op)?;
                if let Some(merged) = self.try_merge_node(child_idx) {
                    *self.nodes[node_idx as usize].child_mut(which_bit) = merged;
                }
            }
            Record::FixedNode(child_idx) => {
                // Never merge a FixedNode — its presence is load-bearing for the reader's
                // IPv4-start walk.
                self.insert_at_node(child_idx, new_depth, bits, prefix_len, op)?;
            }
            Record::Alias(_) | Record::Reserved => {
                // Broadcasting over an alias or reserved leaf leaves it in place (the writer
                // rejects inserts that target aliased/reserved space up front).
            }
            Record::NoData => {
                if new_depth > prefix_len {
                    // At or past the prefix end: apply the operation to an empty leaf.
                    if let Some(id) = op(None) {
                        *self.nodes[node_idx as usize].child_mut(which_bit) = Record::Data(id);
                    }
                } else {
                    // Above the prefix: descend by creating an empty node.
                    let new_idx = self.alloc_node(Node::default())?;
                    *self.nodes[node_idx as usize].child_mut(which_bit) = Record::Node(new_idx);
                    self.insert_at_node(new_idx, new_depth, bits, prefix_len, op)?;
                    if let Some(merged) = self.try_merge_node(new_idx) {
                        *self.nodes[node_idx as usize].child_mut(which_bit) = merged;
                    }
                }
            }
            Record::Data(old_id) => {
                if new_depth > prefix_len {
                    // At or past the prefix end: apply the operation to the existing leaf.
                    let record = match op(Some(old_id)) {
                        Some(id) => Record::Data(id),
                        None => Record::NoData,
                    };
                    *self.nodes[node_idx as usize].child_mut(which_bit) = record;
                } else {
                    // Split: a node whose both children carry the old value, then recurse so
                    // the operation overrides exactly one path.
                    let new_node = Node {
                        left: Record::Data(old_id),
                        right: Record::Data(old_id),
                    };
                    let new_idx = self.alloc_node(new_node)?;
                    *self.nodes[node_idx as usize].child_mut(which_bit) = Record::Node(new_idx);
                    self.insert_at_node(new_idx, new_depth, bits, prefix_len, op)?;
                    if let Some(merged) = self.try_merge_node(new_idx) {
                        *self.nodes[node_idx as usize].child_mut(which_bit) = merged;
                    }
                }
            }
        }
        Ok(())
    }

    /// If node `idx` has two identical leaf records, return that record so the caller can
    /// collapse its pointer to it. Keeps the tree compact during insertion.
    fn try_merge_node(&self, idx: u32) -> Option<Record> {
        let node = self.nodes[idx as usize];
        if node.left == node.right && is_leaf_record(node.left) {
            Some(node.left)
        } else {
            None
        }
    }

    fn alloc_node(&mut self, node: Node) -> Result<u32, Error> {
        let new_idx = u32::try_from(self.nodes.len()).map_err(|_| Error::TreeTooLarge {
            node_count: self.nodes.len(),
            max: u64::from(u32::MAX),
            record_size: RecordSize::Bits32,
        })?;
        self.nodes.push(node);
        Ok(new_idx)
    }

    /// Promote the IPv4 subtree root at `::/96` to a [`Record::FixedNode`] and install the
    /// three alias records pointing at it. No-op if the tree has no IPv4 subtree.
    ///
    /// Call once, after all user inserts and before serialization. Only meaningful for V6
    /// trees.
    pub(crate) fn install_ipv4_aliases(&mut self) -> Result<(), Error> {
        // Walk 96 left children from the root. If the path breaks, there is no v4 subtree.
        let mut cursor: u32 = 0;
        for _ in 0..96 {
            match self.nodes[cursor as usize].left {
                Record::Node(i) => cursor = i,
                _ => return Ok(()),
            }
        }
        let v4_root_idx = cursor;

        // Promote: walk 95 left children to the v4 root's parent and pin its `.left`.
        let mut parent: u32 = 0;
        for _ in 0..95 {
            if let Record::Node(i) = self.nodes[parent as usize].left {
                parent = i;
            } else {
                return Ok(());
            }
        }
        self.nodes[parent as usize].left = Record::FixedNode(v4_root_idx);

        // `::ffff:0:0/96` — IPv4-mapped IPv6 (RFC 4291).
        self.install_marker(
            0x0000_0000_0000_0000_0000_FFFF_0000_0000u128,
            96,
            Record::Alias(v4_root_idx),
        )?;
        // `2001::/32` — Teredo (RFC 4380).
        self.install_marker(
            0x2001_0000_0000_0000_0000_0000_0000_0000u128,
            32,
            Record::Alias(v4_root_idx),
        )?;
        // `2002::/16` — 6to4 (RFC 3056).
        self.install_marker(
            0x2002_0000_0000_0000_0000_0000_0000_0000u128,
            16,
            Record::Alias(v4_root_idx),
        )?;
        Ok(())
    }

    /// Paint a reserved-network leaf covering `(bits, prefix_len)`. Called before user
    /// inserts, on an otherwise-empty subtree.
    pub(crate) fn paint_reserved(&mut self, bits: u128, prefix_len: u8) -> Result<(), Error> {
        self.install_marker(bits, prefix_len, Record::Reserved)
    }

    /// Walk `bits` for `prefix_len` bits, creating intermediate empty nodes, then set the
    /// terminal record to `marker`. Skips cleanly if the path is already occupied by a leaf.
    fn install_marker(&mut self, bits: u128, prefix_len: u8, marker: Record) -> Result<(), Error> {
        let mut node_idx: u32 = 0;
        for bit_index in 0..prefix_len {
            let bit = ((bits >> (127 - bit_index)) & 1) as u8;
            let is_last = bit_index == prefix_len - 1;

            if is_last {
                if let Record::NoData = self.nodes[node_idx as usize].child(bit) {
                    *self.nodes[node_idx as usize].child_mut(bit) = marker;
                }
                return Ok(());
            }

            match self.nodes[node_idx as usize].child(bit) {
                Record::Node(i) | Record::FixedNode(i) => node_idx = i,
                Record::NoData => {
                    let new_idx = self.alloc_node(Node::default())?;
                    *self.nodes[node_idx as usize].child_mut(bit) = Record::Node(new_idx);
                    node_idx = new_idx;
                }
                Record::Data(_) | Record::Alias(_) | Record::Reserved => return Ok(()),
            }
        }
        Ok(())
    }

    /// Look up the value id currently stored for `(bits, prefix_len)`'s first address, if the
    /// exact covering leaf holds data. Used by `Writer::get`.
    pub(crate) fn get(&self, bits: u128, depth_limit: u8) -> Option<ValueId> {
        let mut node_idx: u32 = 0;
        for depth in 0..depth_limit {
            let bit = ((bits >> (127 - depth)) & 1) as u8;
            match self.nodes[node_idx as usize].child(bit) {
                Record::Node(i) | Record::FixedNode(i) | Record::Alias(i) => node_idx = i,
                Record::Data(id) => return Some(id),
                Record::NoData | Record::Reserved => return None,
            }
        }
        None
    }

    /// Value ids still reachable from the root, in a deterministic order (first appearance in
    /// a pre-order walk). Call after [`compact`](Self::compact).
    pub(crate) fn reachable_data_ids(&self) -> Vec<ValueId> {
        let mut seen = HashSet::new();
        let mut ids = Vec::new();
        for node in &self.nodes {
            for record in [node.left, node.right] {
                if let Record::Data(id) = record {
                    if seen.insert(id) {
                        ids.push(id);
                    }
                }
            }
        }
        ids
    }

    /// Check whether the (compacted) tree plus a data section of `data_section_len` bytes
    /// fits inside `record_size`.
    pub(crate) fn fits_record_size(
        &self,
        record_size: RecordSize,
        data_section_len: usize,
    ) -> bool {
        let node_count = self.nodes.len() as u64;
        let max_data_value = (node_count + 16) + data_section_len as u64;
        max_data_value < record_size.max_value()
    }

    /// Serialize the tree to record-packed bytes, resolving each data leaf through
    /// `id_to_offset`. Run [`compact`](Self::compact) first.
    pub(crate) fn serialize(
        &self,
        record_size: RecordSize,
        id_to_offset: &HashMap<ValueId, DataOffset>,
    ) -> Result<Vec<u8>, Error> {
        let node_count = self.nodes.len();
        let mut out = Vec::with_capacity(node_count * record_size.node_bytes());
        for node in &self.nodes {
            let left = encode_record(node.left, node_count, record_size, id_to_offset)?;
            let right = encode_record(node.right, node_count, record_size, id_to_offset)?;
            write_record_pair(&mut out, left, right, record_size);
        }
        Ok(out)
    }

    /// Collapse subtrees whose children resolve to the same leaf, then renumber surviving
    /// nodes contiguously. Idempotent. [`FixedNode`](Record::FixedNode)s are never collapsed;
    /// [`Alias`](Record::Alias)es are treated as leaves.
    pub(crate) fn compact(&mut self) {
        let root_effective = self.compact_node(0);
        if !matches!(root_effective, Record::Node(_) | Record::FixedNode(_)) {
            self.nodes[0] = Node {
                left: root_effective,
                right: root_effective,
            };
        }
        self.renumber_reachable();
    }

    fn compact_node(&mut self, idx: u32) -> Record {
        let left = self.resolve_child(self.nodes[idx as usize].left);
        let right = self.resolve_child(self.nodes[idx as usize].right);
        self.nodes[idx as usize].left = left;
        self.nodes[idx as usize].right = right;

        if left == right && is_leaf_record(left) {
            return left;
        }
        Record::Node(idx)
    }

    fn resolve_child(&mut self, record: Record) -> Record {
        match record {
            Record::Node(i) => self.compact_node(i),
            Record::FixedNode(i) => {
                // Collapse its internals but keep the marker.
                let _ = self.compact_node(i);
                Record::FixedNode(i)
            }
            other => other,
        }
    }

    fn renumber_reachable(&mut self) {
        let mut old_to_new: HashMap<u32, u32> = HashMap::new();
        let mut order: Vec<u32> = Vec::new();
        // Pre-order DFS, left before right (push right first so left pops first). Aliases are
        // not pushed — their target is reached via the FixedNode walk.
        let mut stack: Vec<u32> = vec![0];
        while let Some(old_idx) = stack.pop() {
            if old_to_new.contains_key(&old_idx) {
                continue;
            }
            let new_idx = u32::try_from(order.len()).unwrap_or(u32::MAX);
            old_to_new.insert(old_idx, new_idx);
            order.push(old_idx);
            let node = self.nodes[old_idx as usize];
            if let Record::Node(i) | Record::FixedNode(i) = node.right {
                stack.push(i);
            }
            if let Record::Node(i) | Record::FixedNode(i) = node.left {
                stack.push(i);
            }
        }

        let mut new_nodes: Vec<Node> = Vec::with_capacity(order.len());
        for &old_idx in &order {
            let mut node = self.nodes[old_idx as usize];
            node.left = remap_record(node.left, &old_to_new);
            node.right = remap_record(node.right, &old_to_new);
            new_nodes.push(node);
        }
        self.nodes = new_nodes;
    }
}

fn is_leaf_record(record: Record) -> bool {
    matches!(
        record,
        Record::NoData | Record::Data(_) | Record::Alias(_) | Record::Reserved
    )
}

fn remap_record(record: Record, old_to_new: &HashMap<u32, u32>) -> Record {
    match record {
        Record::Node(i) => old_to_new
            .get(&i)
            .copied()
            .map_or(Record::NoData, Record::Node),
        Record::FixedNode(i) => old_to_new
            .get(&i)
            .copied()
            .map_or(Record::NoData, Record::FixedNode),
        Record::Alias(i) => old_to_new
            .get(&i)
            .copied()
            .map_or(Record::NoData, Record::Alias),
        other => other,
    }
}

fn encode_record(
    record: Record,
    node_count: usize,
    record_size: RecordSize,
    id_to_offset: &HashMap<ValueId, DataOffset>,
) -> Result<u64, Error> {
    let node_count_u64 = node_count as u64;
    let raw = match record {
        Record::NoData | Record::Reserved => node_count_u64,
        Record::Node(i) | Record::FixedNode(i) | Record::Alias(i) => u64::from(i),
        Record::Data(id) => {
            let offset = id_to_offset
                .get(&id)
                .copied()
                .expect("every reachable data id has an offset");
            // `+16` skips the reserved range the spec carves out after the node section.
            node_count_u64 + 16 + u64::from(offset)
        }
    };
    if raw >= record_size.max_value() {
        return Err(Error::TreeTooLarge {
            node_count,
            max: record_size.max_value(),
            record_size,
        });
    }
    Ok(raw)
}

fn write_record_pair(out: &mut Vec<u8>, left: u64, right: u64, record_size: RecordSize) {
    match record_size {
        RecordSize::Bits24 => {
            out.push((left >> 16) as u8);
            out.push((left >> 8) as u8);
            out.push(left as u8);
            out.push((right >> 16) as u8);
            out.push((right >> 8) as u8);
            out.push(right as u8);
        }
        RecordSize::Bits28 => {
            // 7-byte node layout: low 24 bits of LEFT, a middle byte packing the two high
            // nibbles (left in the high nibble, right in the low nibble), low 24 bits of
            // RIGHT.
            let left_high4 = ((left >> 24) & 0xF) as u8;
            let right_high4 = ((right >> 24) & 0xF) as u8;
            out.push((left >> 16) as u8);
            out.push((left >> 8) as u8);
            out.push(left as u8);
            out.push((left_high4 << 4) | right_high4);
            out.push((right >> 16) as u8);
            out.push((right >> 8) as u8);
            out.push(right as u8);
        }
        RecordSize::Bits32 => {
            out.push((left >> 24) as u8);
            out.push((left >> 16) as u8);
            out.push((left >> 8) as u8);
            out.push(left as u8);
            out.push((right >> 24) as u8);
            out.push((right >> 16) as u8);
            out.push((right >> 8) as u8);
            out.push(right as u8);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Insert with plain replacement semantics, storing `id` at every covered leaf.
    fn insert_set(tree: &mut Tree, bits: u128, prefix_len: u8, id: ValueId) {
        tree.insert(bits, prefix_len, &mut |_| Some(id)).unwrap();
    }

    fn v4_bits(a: u8, b: u8, c: u8, d: u8) -> u128 {
        // Under `::/96`: low 32 bits carry the address.
        u128::from(u32::from_be_bytes([a, b, c, d]))
    }

    #[test]
    fn single_v4_24_compacts_to_expected_nodes() {
        let mut tree = Tree::new();
        // 1.2.3.0/24 under ::/96 → prefix length 96 + 24 = 120.
        insert_set(&mut tree, v4_bits(1, 2, 3, 0), 120, 0);
        tree.compact();
        // A single prefix collapses to one path of length prefix_len with a data leaf at the
        // end: exactly `prefix_len` nodes (root + one per bit above the leaf).
        assert_eq!(tree.node_count(), 120);
        // The value is reachable.
        assert_eq!(tree.reachable_data_ids(), vec![0]);
    }

    #[test]
    fn compact_is_idempotent() {
        let mut tree = Tree::new();
        insert_set(&mut tree, v4_bits(10, 0, 0, 0), 104, 0); // 10.0.0.0/8
        insert_set(&mut tree, v4_bits(10, 1, 0, 0), 112, 1); // 10.1.0.0/16
        tree.compact();
        let count = tree.node_count();
        tree.compact();
        assert_eq!(tree.node_count(), count);
    }

    #[test]
    fn more_specific_after_less_specific_preserves_siblings() {
        let mut tree = Tree::new();
        insert_set(&mut tree, v4_bits(10, 0, 0, 0), 104, 0); // 10.0.0.0/8 → id 0
        insert_set(&mut tree, v4_bits(10, 1, 0, 0), 112, 1); // 10.1.0.0/16 → id 1
        tree.compact();
        // 10.1.2.3 resolves to id 1, 10.2.2.3 still resolves to id 0.
        assert_eq!(tree.get(v4_bits(10, 1, 2, 3), 128), Some(1));
        assert_eq!(tree.get(v4_bits(10, 2, 2, 3), 128), Some(0));
    }

    #[test]
    fn less_specific_after_more_specific_overwrites_whole_range() {
        let mut tree = Tree::new();
        insert_set(&mut tree, v4_bits(10, 1, 0, 0), 112, 1); // 10.1.0.0/16 → id 1
        insert_set(&mut tree, v4_bits(10, 0, 0, 0), 104, 0); // 10.0.0.0/8 → id 0 (paints all)
        tree.compact();
        assert_eq!(tree.get(v4_bits(10, 1, 2, 3), 128), Some(0));
        assert_eq!(tree.reachable_data_ids(), vec![0]);
    }

    #[test]
    fn zero_prefix_paints_everything() {
        let mut tree = Tree::new();
        insert_set(&mut tree, 0, 0, 7); // ::/0
        tree.compact();
        assert_eq!(tree.get(v4_bits(1, 2, 3, 4), 128), Some(7));
        assert_eq!(tree.get(0xDEAD << 112, 128), Some(7));
        // Everything collapses to the root's two data records.
        assert_eq!(tree.node_count(), 1);
    }

    #[test]
    fn twenty_eight_bit_nibble_packing() {
        // Left = 0x1ABCDEF, right = 0x2FEDCBA (both < 2^28). The middle byte packs the high
        // nibbles: 0x1 (left) << 4 | 0x2 (right) = 0x12.
        let mut out = Vec::new();
        write_record_pair(&mut out, 0x1AB_CDEF, 0x2FE_DCBA, RecordSize::Bits28);
        assert_eq!(out, vec![0xAB, 0xCD, 0xEF, 0x12, 0xFE, 0xDC, 0xBA]);
    }
}
