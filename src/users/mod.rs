//! Portable workspace members and their normalized contact identities.

mod assignment;
mod command;
mod id;
mod model;
mod normalize;
mod store;
mod transaction;
mod validate;

pub use assignment::AssignmentRewrites;
pub use command::{
    LegacyUserMigration, UserMutation, apply_mutation, propose_legacy_user_migration,
    proposed_user_id,
};
pub use id::{UserId, UserIdError};
pub use model::{EmailIdentity, PhoneIdentity, USERS_SCHEMA_VERSION, User, Users};
pub use normalize::{NormalizeError, normalize_email, normalize_mailbox, normalize_phone};
pub use store::UsersStore;
pub(crate) use transaction::{FileChange, replace_group};
pub use validate::UsersError;
