use super::*;
use crate::state::Db;

mod support;

mod identity {
    use super::support::*;
    use super::*;

    include!("tests/identity.rs");
}

mod acceptance {
    use super::support::*;
    use super::*;

    include!("tests/acceptance.rs");
}

mod claims {
    use super::support::*;
    use super::*;

    include!("tests/claims.rs");
}

mod conversation {
    use super::support::*;
    use super::*;

    include!("tests/conversation.rs");
}

mod recovery {
    use super::support::*;
    use super::*;

    include!("tests/recovery.rs");
}

mod recovery_policy {
    use super::*;

    include!("tests/recovery_policy.rs");
}

mod recovery_state {
    use super::support::*;
    use super::*;

    include!("tests/recovery_state.rs");
}

mod recovery_claim {
    use super::support::*;
    use super::*;

    include!("tests/recovery_claim.rs");
    include!("tests/recovery_claim_ordering.rs");
}

mod unavailable_notice {
    use super::support::*;
    use super::*;

    include!("tests/unavailable_notice.rs");
}

mod reconciliation {
    use super::support::*;
    use super::*;

    include!("tests/reconciliation.rs");
}

mod binding {
    use super::support::*;
    use super::*;

    include!("tests/binding.rs");
}

mod launch {
    use super::support::*;
    use super::*;

    include!("tests/launch.rs");
}

mod privacy {
    use super::support::*;
    use super::*;

    include!("tests/privacy.rs");
}

mod delivery_model {
    use super::support::*;
    use super::*;

    include!("tests/delivery_model.rs");
}

mod delivery_policy {
    use super::*;

    include!("tests/delivery_policy.rs");
}

mod schema {
    use super::support::*;
    use super::*;

    include!("tests/schema.rs");
    include!("tests/schema_sections/collisions.rs");
    include!("tests/schema_sections/downgrade.rs");
    include!("tests/schema_sections/delivery.rs");
    include!("tests/schema_sections/delivery_downgrade_v11.rs");
    include!("tests/schema_sections/delivery_repair.rs");
    include!("tests/schema_sections/delivery_writer.rs");
    include!("tests/schema_sections/writer_ordering.rs");
}
