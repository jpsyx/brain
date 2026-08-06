use brain::workspace::{RequiredStatus, RequirementScope, requirements};
use serde_json::Map;

use super::support::{Fixture, required_status};

#[test]
fn unavailable_root_is_required_workspace_unavailability() {
    let fixture = Fixture::new(Map::new());
    std::fs::rename(
        fixture.command.workspace.root(),
        fixture.command.workspace.root().with_extension("offline"),
    )
    .expect("make selected root unavailable");

    let health = requirements(&fixture.command).expect("inspect selected workspace");

    assert_eq!(
        required_status(&health, &RequirementScope::WorkspaceRoot),
        RequiredStatus::Unavailable
    );
}

#[test]
fn ready_workspace_reports_every_required_availability_invariant() {
    let fixture = Fixture::new(Map::new());

    let health = requirements(&fixture.command).expect("inspect selected workspace");

    for scope in [
        RequirementScope::WorkspaceRoot,
        RequirementScope::WorkspaceManifest,
        RequirementScope::PortableUsers,
        RequirementScope::LocalUser,
    ] {
        assert_eq!(
            required_status(&health, &scope),
            RequiredStatus::Ready,
            "{scope:?}"
        );
    }
}
