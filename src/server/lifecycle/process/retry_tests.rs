use super::{retry_after_starter_wait, retry_winner_election};
use std::time::{Duration, Instant};

#[test]
fn owner_cleanup_while_waiting_returns_the_loser_to_election() {
    let now = Instant::now();
    assert!(retry_winner_election(
        false,
        now,
        now + Duration::from_secs(1)
    ));
}

#[test]
fn starter_failure_before_publication_returns_the_owner_to_election() {
    let now = Instant::now();
    assert!(retry_after_starter_wait(
        true,
        now,
        now + Duration::from_secs(1)
    ));
}
