use super::package::ManagedWorkspacePackage;
use super::{Checkpoint, InstallIdentity, InstallVersion, LockedInstall, NoStamp};
use crate::{skills::transaction_tests::rendered, workspace::instructions::plan_document};
use std::{fs, path::Path};

pub(super) fn package(root: &Path, body: &str) -> ManagedWorkspacePackage {
    let before = fs::read(root.join("AGENTS.md")).ok();
    let prior = before.as_deref().map(|_| {
        plan_document(None, "old instructions", None)
            .unwrap()
            .receipt
    });
    ManagedWorkspacePackage {
        skills: rendered(body),
        instructions: Some(
            plan_document(
                before.as_deref(),
                &format!("{body} instructions"),
                prior.as_ref(),
            )
            .unwrap(),
        ),
        config_patches: Vec::new(),
        source_patches: Vec::new(),
    }
}

#[test]
fn first_published_artifact_failure_restores_instructions_skills_extras_and_frontends() {
    let root = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let identity = InstallIdentity::sandbox_with_cache(root.path(), cache.path()).unwrap();
    let mut locked = LockedInstall::acquire(&identity).unwrap();
    let version = InstallVersion::current();
    fs::write(root.path().join("AGENTS.md"), "User prefix\n").unwrap();
    locked
        .install_package(package(root.path(), "old"), &version, &mut NoStamp)
        .unwrap();
    let old = fs::read(root.path().join("AGENTS.md")).unwrap();
    assert!(String::from_utf8_lossy(&old).contains("old instructions"));
    let extra = root.path().join(".agents/skills/todo/custom.txt");
    fs::write(&extra, "custom").unwrap();
    assert!(
        locked
            .install_package_with(
                package(root.path(), "new"),
                &version,
                &mut NoStamp,
                &mut |point| {
                    anyhow::ensure!(point != Checkpoint::Installed(0), "injected interruption");
                    Ok(())
                }
            )
            .is_err()
    );
    locked.recover(&mut NoStamp).unwrap();
    assert_eq!(fs::read(root.path().join("AGENTS.md")).unwrap(), old);
    assert_eq!(fs::read(extra).unwrap(), b"custom");
    for parent in [".agents", ".claude", ".codex", ".opencode"] {
        assert_eq!(
            fs::read(root.path().join(parent).join("skills/todo/SKILL.md")).unwrap(),
            b"old"
        );
        assert_eq!(
            fs::read(
                root.path()
                    .join(parent)
                    .join("skills/todo/references/guide.md")
            )
            .unwrap(),
            b"reference old"
        );
    }
}

#[test]
fn currentness_owns_only_the_instruction_region_and_skill_only_updates_keep_ownership() {
    use super::Currentness;
    let root = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let identity = InstallIdentity::sandbox_with_cache(root.path(), cache.path()).unwrap();
    let mut locked = LockedInstall::acquire(&identity).unwrap();
    let version = InstallVersion::current();
    locked
        .install_package(package(root.path(), "old"), &version, &mut NoStamp)
        .unwrap();
    let agents = root.path().join("AGENTS.md");
    let bytes = fs::read(&agents).unwrap();
    fs::write(
        &agents,
        [b"new user prefix\n".as_slice(), &bytes, b"user suffix\n"].concat(),
    )
    .unwrap();
    assert!(matches!(
        locked.inspect(&version).unwrap(),
        Currentness::Current(_)
    ));
    locked
        .install(rendered("old"), &version, &mut NoStamp)
        .unwrap();
    fs::write(
        &agents,
        String::from_utf8(fs::read(&agents).unwrap())
            .unwrap()
            .replace("old instructions", "modified region"),
    )
    .unwrap();
    assert!(
        locked.inspect(&version).is_err(),
        "modified owned body must conflict after skill-only refresh"
    );
}

#[test]
fn instruction_auth_rejects_an_emptied_managed_body_instead_of_treating_it_as_absent() {
    let root = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let identity = InstallIdentity::sandbox_with_cache(root.path(), cache.path()).unwrap();
    let mut locked = LockedInstall::acquire(&identity).unwrap();
    let version = InstallVersion::current();
    locked
        .install_package(package(root.path(), "old"), &version, &mut NoStamp)
        .unwrap();
    let agents = root.path().join("AGENTS.md");
    fs::write(
        &agents,
        fs::read_to_string(&agents)
            .unwrap()
            .replace("old instructions", ""),
    )
    .unwrap();
    assert!(
        locked.inspect(&version).is_err(),
        "an empty modified region is still owned content"
    );
}

#[test]
fn instruction_auth_adopts_exact_supported_render_with_a_stale_local_receipt() {
    let root = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let identity = InstallIdentity::sandbox_with_cache(root.path(), cache.path()).unwrap();
    let mut locked = LockedInstall::acquire(&identity).unwrap();
    let version = InstallVersion::current();
    locked
        .install_package(package(root.path(), "old"), &version, &mut NoStamp)
        .unwrap();
    let agents = root.path().join("AGENTS.md");
    let desired = plan_document(None, "new instructions", None).unwrap().after;
    fs::write(&agents, desired).unwrap();
    let target = package(root.path(), "new");
    locked.preflight_package(&target, &version).unwrap();
    locked
        .install_package(target, &version, &mut NoStamp)
        .unwrap();
    assert!(matches!(
        locked.inspect(&version).unwrap(),
        super::Currentness::Current(_)
    ));
}
