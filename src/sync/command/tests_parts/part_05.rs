
#[test]
fn flags_map_to_direction() {
    assert_eq!(direction_from_flags(false, false).unwrap(), Direction::Both);
    assert_eq!(direction_from_flags(true, false).unwrap(), Direction::Push);
    assert_eq!(direction_from_flags(false, true).unwrap(), Direction::Pull);
    assert!(direction_from_flags(true, true).is_err());
}

#[test]
fn auto_resyncs_only_on_prior_listing_missing_and_not_already_a_resync() {
    use crate::sync::run::AbortKind;
    assert!(should_auto_resync(
        Direction::Both,
        Some(&AbortKind::PriorListingMissing)
    ));
    assert!(should_auto_resync(
        Direction::Push,
        Some(&AbortKind::PriorListingMissing)
    ));
    // already a resync -> don't loop
    assert!(!should_auto_resync(
        Direction::Resync,
        Some(&AbortKind::PriorListingMissing)
    ));
    // other aborts / clean -> no auto resync
    assert!(!should_auto_resync(
        Direction::Both,
        Some(&AbortKind::MaxDelete)
    ));
    assert!(!should_auto_resync(Direction::Both, None));
}

#[test]
fn check_access_abort_is_auto_repaired_once_for_normal_syncs() {
    use crate::sync::run::AbortKind;
    assert!(should_auto_repair_check_access(
        Direction::Both,
        Some(&AbortKind::CheckAccess)
    ));
    assert!(should_auto_repair_check_access(
        Direction::Push,
        Some(&AbortKind::CheckAccess)
    ));
    assert!(!should_auto_repair_check_access(
        Direction::Resync,
        Some(&AbortKind::CheckAccess)
    ));
    assert!(!should_auto_repair_check_access(
        Direction::Both,
        Some(&AbortKind::PriorListingMissing)
    ));
}

#[test]
fn check_access_bootstrap_runs_only_for_resync() {
    assert!(should_bootstrap_check_access(Direction::Resync));
    assert!(!should_bootstrap_check_access(Direction::Both));
    assert!(!should_bootstrap_check_access(Direction::Push));
    assert!(!should_bootstrap_check_access(Direction::Pull));
}
