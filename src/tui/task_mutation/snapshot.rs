//! Worker snapshot ordering relative to independently adopted authoritative rows.
#[derive(Default)]
pub(crate) struct SnapshotOrder {
    revision: u64,
}
impl SnapshotOrder {
    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }
    pub(crate) fn refreshed(&mut self) {
        self.revision += 1;
    }
    pub(crate) fn accepts(&self, revision: u64) -> bool {
        self.revision == revision
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_revision_rejects_intervening_refresh_but_accepts_recaptured_evidence() {
        let mut order = SnapshotOrder::default();
        let queued = order.revision();
        assert!(order.accepts(queued));
        order.refreshed();
        assert!(!order.accepts(queued));
        let fresh = order.revision();
        assert!(order.accepts(fresh));
        assert!(
            order.accepts(fresh),
            "FIFO worker adoption does not invalidate its successors"
        );
        order.refreshed();
        assert!(!order.accepts(fresh));
    }
}
