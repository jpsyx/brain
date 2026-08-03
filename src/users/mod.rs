//! Portable workspace members and their normalized contact identities.

mod command;
mod id;
mod model;
mod normalize;
mod store;
mod validate;

pub use command::{
    LegacyUserMigration, UserMutation, apply_mutation, propose_legacy_user_migration,
    proposed_user_id,
};
pub use id::{UserId, UserIdError};
pub use model::{EmailIdentity, PhoneIdentity, USERS_SCHEMA_VERSION, User, Users};
pub use normalize::{NormalizeError, normalize_email, normalize_phone};
pub use store::UsersStore;
pub use validate::UsersError;
