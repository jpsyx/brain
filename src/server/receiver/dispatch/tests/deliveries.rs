use std::sync::{Arc, Barrier, Mutex};

use super::super::deliveries::{
    DELIVERIES, ProviderDeliveries, ProviderKey, forward_provider_delivery,
    provider_delivery_was_discarded, remember_verified_unavailable_email,
};
use crate::server::receiver::Channel;
use crate::workspace::WorkspaceId;

#[test]
fn every_nonconcurrent_provider_retry_reaches_durable_acceptance() {
    let deliveries = Mutex::new(ProviderDeliveries::default());
    let key = key(PERSONAL_ID, Channel::Sms, "provider-1");
    let mut attempts = 0;

    assert!(
        forward_provider_delivery(&deliveries, &key, || {
            attempts += 1;
            anyhow::bail!("durable receiver state unavailable")
        })
        .is_err()
    );
    forward_provider_delivery(&deliveries, &key, || {
        attempts += 1;
        Ok(())
    })
    .unwrap();
    forward_provider_delivery(&deliveries, &key, || {
        attempts += 1;
        Ok(())
    })
    .unwrap();

    assert_eq!(attempts, 3);
}

#[test]
fn completed_durable_delivery_still_rechecks_persistence_on_retry() {
    let deliveries = Mutex::new(ProviderDeliveries::default());
    let key = key(PERSONAL_ID, Channel::Sms, "provider-durable-retry");
    let mut durable_checks = 0;

    forward_provider_delivery(&deliveries, &key, || {
        durable_checks += 1;
        Ok(())
    })
    .unwrap();
    forward_provider_delivery(&deliveries, &key, || {
        durable_checks += 1;
        Ok(())
    })
    .unwrap();

    assert_eq!(durable_checks, 2);
}

#[test]
fn successful_durable_email_is_not_remembered_as_an_unavailable_discard() {
    let deliveries = Mutex::new(ProviderDeliveries::default());
    let key = key(PERSONAL_ID, Channel::Email, "provider-durable-email");

    forward_provider_delivery(&deliveries, &key, || Ok(())).unwrap();

    assert!(
        deliveries.lock().unwrap().begin(key).started(),
        "successful durable Email would bypass persistence during authentication"
    );
}

#[test]
fn verified_email_unavailability_is_remembered_as_a_discarded_delivery() {
    let mut deliveries = ProviderDeliveries::default();
    let key = key(PERSONAL_ID, Channel::Email, "verified-unavailable-email");

    assert!(deliveries.begin(key.clone()).started());
    deliveries.finish(&key, true);

    assert!(
        !deliveries.begin(key).started(),
        "a verified unavailable Resend event was replayable"
    );
}

#[test]
fn in_flight_duplicate_is_not_acknowledged_before_first_handoff_finishes() {
    let deliveries = Arc::new(Mutex::new(ProviderDeliveries::default()));
    let key = key(PERSONAL_ID, Channel::Email, "provider-2");
    let entered = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    let worker_deliveries = Arc::clone(&deliveries);
    let worker_key = key.clone();
    let worker_entered = Arc::clone(&entered);
    let worker_release = Arc::clone(&release);
    let worker = std::thread::spawn(move || {
        forward_provider_delivery(&worker_deliveries, &worker_key, || {
            worker_entered.wait();
            worker_release.wait();
            Ok(())
        })
    });
    entered.wait();

    assert!(forward_provider_delivery(&deliveries, &key, || Ok(())).is_err());
    release.wait();
    worker.join().unwrap().unwrap();
    forward_provider_delivery(&deliveries, &key, || Ok(())).unwrap();
}

#[test]
fn verified_unavailable_email_is_retained_when_in_flight_acceptance_fails() {
    let key = key(
        PERSONAL_ID,
        Channel::Email,
        "verified-unavailable-during-acceptance",
    );
    let entered = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    let worker_key = key.clone();
    let worker_entered = Arc::clone(&entered);
    let worker_release = Arc::clone(&release);
    let worker = std::thread::spawn(move || {
        forward_provider_delivery(&DELIVERIES, &worker_key, || {
            worker_entered.wait();
            worker_release.wait();
            anyhow::bail!("durable acceptance lost workspace authority")
        })
    });
    entered.wait();

    let acknowledge_unavailable = remember_verified_unavailable_email(key.0, key.2.clone());
    let duplicate_while_pending = forward_provider_delivery(&DELIVERIES, &key, || Ok(()));
    release.wait();
    let original = worker.join().unwrap();

    assert!(original.is_err());
    assert!(
        !acknowledge_unavailable,
        "verified duplicate was acknowledged before in-flight acceptance resolved"
    );
    assert!(duplicate_while_pending.is_err());
    assert!(
        provider_delivery_was_discarded(key.0, key.1, &key.2),
        "verified unavailable Email was replayable after in-flight acceptance failed"
    );
}

#[test]
fn retained_unavailable_discards_are_bounded_and_workspace_channel_scoped() {
    let mut deliveries = ProviderDeliveries::default();
    for index in 0..=1024 {
        let key = key(PERSONAL_ID, Channel::Sms, &format!("provider-{index}"));
        assert!(deliveries.begin(key.clone()).started());
        deliveries.finish(&key, true);
    }

    assert!(
        deliveries
            .begin(key(PERSONAL_ID, Channel::Sms, "provider-0"))
            .started()
    );
    assert!(
        deliveries
            .begin(key(FAMILY_ID, Channel::Sms, "provider-1024"))
            .started()
    );
    assert!(
        deliveries
            .begin(key(PERSONAL_ID, Channel::Email, "provider-1024"))
            .started()
    );
}

const PERSONAL_ID: &str = "8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b";
const FAMILY_ID: &str = "e806258e-491a-436d-9db4-a5ca9903e0d4";

fn key(workspace: &str, channel: Channel, provider_id: &str) -> ProviderKey {
    (
        WorkspaceId::parse(workspace).unwrap(),
        channel,
        provider_id.to_owned(),
    )
}
