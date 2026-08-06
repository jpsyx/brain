use anyhow::Result;

use super::{PromptMetadata, RequiredStatus, Requirement, RequirementScope};
use crate::workspace::CommandContext;

/// Inspect one immutable selected workspace without consulting peer records.
pub fn requirements(command: &CommandContext) -> Result<Vec<Requirement>> {
    let registry = crate::workspace::RegistryStore::load_from(command.registry_store.path())?;
    let selected = registry.select(Some(command.workspace.name().as_str()))?;
    anyhow::ensure!(
        selected.record().workspace_id == command.workspace.id(),
        "selected workspace identity changed while inspecting requirements"
    );
    let name = selected.canonical_name().as_str();
    let root = command.workspace.root();
    let root_status = ready_required(root.is_dir());
    let manifest_status = ready_required(
        crate::workspace::WorkspaceManifest::load(root, env!("CARGO_PKG_VERSION"))
            .is_ok_and(|manifest| manifest.workspace_id() == command.workspace.id()),
    );
    let users =
        crate::users::UsersStore::load_from(&crate::users::UsersStore::path(&command.workspace));
    let users_status = ready_required(users.as_ref().is_ok_and(|users| !users.users.is_empty()));
    let local_status = ready_required(
        crate::users::UserId::parse(command.workspace.local_user_id())
            .ok()
            .is_some_and(|local_user| {
                users
                    .as_ref()
                    .is_ok_and(|users| users.user(&local_user).is_some())
            }),
    );
    let env = &selected.record().env;
    let (sync_status, watcher_status) = super::sync::statuses(env.get("sync"));
    let (receiver_status, sms_status, email_status) =
        super::receiver::statuses(selected.record().receiver_enabled, env, users.as_ref().ok());

    let mut rows = vec![
        Requirement::required(
            RequirementScope::WorkspaceRoot,
            root_status,
            vec![PromptMetadata::plain("Workspace root")],
            format!("restore the selected workspace root at {}", root.display()),
        ),
        Requirement::required(
            RequirementScope::WorkspaceManifest,
            manifest_status,
            Vec::new(),
            format!("brain workspace repair -b {name} --manifest"),
        ),
        Requirement::required(
            RequirementScope::PortableUsers,
            users_status,
            vec![
                PromptMetadata::plain("User ID"),
                PromptMetadata::plain("Display name"),
            ],
            format!("brain user add -b {name} --id <USER_ID> --name <DISPLAY_NAME>"),
        ),
        Requirement::required(
            RequirementScope::LocalUser,
            local_status,
            vec![PromptMetadata::plain("Local user ID")],
            format!("brain user local <USER_ID> -b {name}"),
        ),
        Requirement::feature(
            RequirementScope::CloudSync,
            sync_status,
            super::sync::prompts(),
            format!("brain sync setup -b {name}"),
        ),
        Requirement::feature(
            RequirementScope::SyncWatcher,
            watcher_status,
            Vec::new(),
            format!("brain env set -b {name} sync.watch=true"),
        ),
        Requirement::feature(
            RequirementScope::Receiver,
            receiver_status,
            Vec::new(),
            format!("brain receiver start -b {name}"),
        ),
        Requirement::feature(
            RequirementScope::Sms,
            sms_status,
            super::receiver::sms_prompts(),
            format!(
                "brain receiver setup -b {name} --channels sms --public-url <HTTPS_URL> --twilio-account-sid <ACCOUNT_SID> --twilio-auth-token <AUTH_TOKEN> --twilio-from-number <E164> --user-id <USER_ID> --phone <E164> --phone-allowed true"
            ),
        ),
        Requirement::feature(
            RequirementScope::Email,
            email_status,
            super::receiver::email_prompts(),
            format!(
                "brain receiver setup -b {name} --channels email --public-url <HTTPS_URL> --resend-api-key <API_KEY> --resend-from-email <FROM_EMAIL> --resend-webhook-signing-secret <SIGNING_SECRET> --user-id <USER_ID> --email <EMAIL> --email-allowed true"
            ),
        ),
    ];
    rows.extend(super::features::requirements(command, name));
    Ok(rows)
}

const fn ready_required(ready: bool) -> RequiredStatus {
    if ready {
        RequiredStatus::Ready
    } else {
        RequiredStatus::Unavailable
    }
}
