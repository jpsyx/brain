//! Portable workspace members and their normalized contact identities.

mod assignment;
mod command;
mod id;
mod model;
mod normalize;
mod select;
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
pub use normalize::{
    NormalizeError, normalize_email, normalize_mailbox, normalize_phone, validate_canonical_mailbox,
};
pub(crate) use select::{Choice, interpret_row, local_user_choices, numbered_rows};
pub use store::UsersStore;
pub(crate) use transaction::{FileChange, replace_group};
pub use validate::UsersError;
