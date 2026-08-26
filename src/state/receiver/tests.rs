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

mod schema {
    use super::support::*;
    use super::*;

    include!("tests/schema.rs");
    include!("tests/schema_sections/collisions.rs");
    include!("tests/schema_sections/downgrade.rs");
}
