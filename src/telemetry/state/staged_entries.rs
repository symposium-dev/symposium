//! Per-entry staging for deterministic private-state maps.

use std::collections::{BTreeMap, btree_map::Entry};

/// Copy-on-write edits to the entries touched by one recording operation.
///
/// When `discard_destination` is set, the destination belongs to a closed UTC
/// day. Base-map lookup is then deliberately disabled: correctness must not
/// depend on every key type carrying the day strongly enough to miss stale
/// entries by accident.
pub(super) struct StagedEntries<'a, K, V> {
    destination: &'a mut BTreeMap<K, V>,
    staged: BTreeMap<K, V>,
    discard_destination: bool,
}

impl<'a, K, V> StagedEntries<'a, K, V>
where
    K: Clone + Ord,
    V: Clone,
{
    pub(super) const fn new(
        destination: &'a mut BTreeMap<K, V>,
        discard_destination: bool,
    ) -> Self {
        Self {
            destination,
            staged: BTreeMap::new(),
            discard_destination,
        }
    }

    /// Borrow a staged entry, cloning or constructing it on first access.
    pub(super) fn get_or_insert_with(&mut self, key: K, create: impl FnOnce(&K) -> V) -> &mut V {
        match self.staged.entry(key) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => {
                let value = if self.discard_destination {
                    None
                } else {
                    self.destination.get(entry.key()).cloned()
                }
                .unwrap_or_else(|| create(entry.key()));
                entry.insert(value)
            }
        }
    }

    /// Apply every prepared entry edit without a recoverable failure.
    ///
    /// Multi-store recording updates commit their stages sequentially. This
    /// method must remain infallible so that sequence cannot stop after only
    /// one private store has committed.
    pub(super) fn commit(mut self) {
        if self.discard_destination {
            self.destination.clear();
        }
        self.destination.append(&mut self.staged);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_access_clones_only_the_selected_entry() {
        let mut destination = BTreeMap::from([(1, vec![1]), (2, vec![2])]);
        let mut staged = StagedEntries::new(&mut destination, false);

        staged.get_or_insert_with(1, |_| Vec::new()).push(3);
        staged.commit();

        assert_eq!(destination, BTreeMap::from([(1, vec![1, 3]), (2, vec![2])]));
    }

    #[test]
    fn dropping_staged_entries_leaves_the_destination_unchanged() {
        let mut destination = BTreeMap::from([(1, vec![1])]);
        let before = destination.clone();
        {
            let mut staged = StagedEntries::new(&mut destination, false);
            staged.get_or_insert_with(1, |_| Vec::new()).push(2);
            staged.get_or_insert_with(2, |_| vec![3]);
        }

        assert_eq!(destination, before);
    }

    #[test]
    fn rollover_never_reads_entries_from_the_closed_day() {
        let mut destination = BTreeMap::from([(1, vec![1])]);
        let mut staged = StagedEntries::new(&mut destination, true);

        let selected = staged.get_or_insert_with(1, |_| vec![2]);

        assert_eq!(selected.as_slice(), &[2]);
        staged.commit();
        assert_eq!(destination, BTreeMap::from([(1, vec![2])]));
    }
}
