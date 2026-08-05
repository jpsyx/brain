use std::sync::{Arc, Barrier, Mutex};

use super::super::{ProviderDeliveries, ProviderKey, forward_provider_delivery};
use crate::server::receiver::Channel;
use crate::workspace::WorkspaceId;

#[test]
fn failed_handoff_releases_provider_id_for_one_later_success() {
    let deliveries = Mutex::new(ProviderDeliveries::default());
    let key = key(PERSONAL_ID, Channel::Sms, "provider-1");
    let mut attempts = 0;

    assert!(
        forward_provider_delivery(&deliveries, &key, || {
            attempts += 1;
            anyhow::bail!("socket unavailable")
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

    assert_eq!(attempts, 2);
}

#[test]
fn verified_email_unavailability_is_remembered_as_a_discarded_delivery() {
    let mut deliveries = ProviderDeliveries::default();
    let key = key(PERSONAL_ID, Channel::Email, "verified-unavailable-email");

    assert!(deliveries.begin(key.clone()).started());
    deliveries.finish(&key, false);

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
    forward_provider_delivery(&deliveries, &key, || {
        panic!("accepted duplicate reached the job socket")
    })
    .unwrap();
}

#[test]
fn retained_provider_ids_are_bounded_and_workspace_channel_scoped() {
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
