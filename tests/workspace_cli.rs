use std::path::{Path, PathBuf};
use std::process::{Command, Output};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use brain::workspace::{MachineRegistry, RegistryStore, WorkspaceName};
use tempfile::TempDir;

struct Fixture {
    home: TempDir,
    config_home: TempDir,
    current_dir: TempDir,
}

impl Fixture {
    fn new() -> Self {
        Self {
            home: tempfile::tempdir().expect("isolated HOME"),
            config_home: tempfile::tempdir().expect("isolated XDG_CONFIG_HOME"),
            current_dir: tempfile::tempdir().expect("isolated current directory"),
        }
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_brain"));
        command
            .env("HOME", self.home.path())
            .env("XDG_CONFIG_HOME", self.config_home.path())
            .env("NO_COLOR", "1")
            .current_dir(self.current_dir.path());
        command
    }

    fn run(&self, args: &[&str]) -> Output {
        self.command().args(args).output().expect("run brain")
    }

    #[cfg(unix)]
    fn barrier_command(&self, release: &Path, args: &[&str]) -> Command {
        let mut command = Command::new("sh");
        command
            .arg("-c")
            .arg("while [ ! -e \"$1\" ]; do :; done; shift; exec \"$@\"")
            .arg("brain-workspace-test")
            .arg(release)
            .arg(env!("CARGO_BIN_EXE_brain"))
            .args(args)
            .env("HOME", self.home.path())
            .env("XDG_CONFIG_HOME", self.config_home.path())
            .env("NO_COLOR", "1")
            .current_dir(self.current_dir.path());
        command
    }

    fn registry_path(&self) -> PathBuf {
        self.config_home.path().join("brain/env.json")
    }

    fn registry(&self) -> MachineRegistry {
        RegistryStore::load_from(&self.registry_path()).expect("valid isolated registry")
    }
}

fn name(value: &str) -> WorkspaceName {
    WorkspaceName::parse(value).expect("valid fixture name")
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "command failed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn assert_failure_contains(output: &Output, expected: &[&str]) {
    assert!(!output.status.success(), "command unexpectedly succeeded");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains('\x1b'),
        "NO_COLOR must suppress ANSI: {stderr:?}"
    );
    for fragment in expected {
        assert!(
            stderr.contains(fragment),
            "missing {fragment:?} in {stderr:?}"
        );
    }
}

fn path_arg(path: &Path) -> &str {
    path.to_str().expect("fixture paths are UTF-8")
}

#[cfg(unix)]
fn fake_markdown_to_pdf(path: &Path) {
    std::fs::write(path, "#!/bin/sh\nexit 0\n").expect("fake markdown-to-pdf");
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
        .expect("make fake markdown-to-pdf executable");
}

#[cfg(unix)]
struct ReadOnlyDir {
    path: PathBuf,
    original_mode: u32,
}

#[cfg(unix)]
impl ReadOnlyDir {
    fn new(path: &Path) -> Self {
        let original_mode = std::fs::metadata(path)
            .expect("directory metadata")
            .permissions()
            .mode();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o500))
            .expect("make registry directory read-only");
        Self {
            path: path.to_path_buf(),
            original_mode,
        }
    }
}

#[cfg(unix)]
impl Drop for ReadOnlyDir {
    fn drop(&mut self) {
        std::fs::set_permissions(
            &self.path,
            std::fs::Permissions::from_mode(self.original_mode),
        )
        .expect("restore registry directory permissions");
    }
}

#[test]
fn first_workspace_create_uses_root_basename_and_becomes_default() {
    let fixture = Fixture::new();
    let root = fixture.home.path().join("Family");

    let output = fixture.run(&["workspace", "create", "--root", path_arg(&root)]);

    assert_success(&output);
    assert!(root.is_dir());
    let registry = fixture.registry();
    assert_eq!(registry.default_workspace, name("family"));
    assert_eq!(registry.workspaces.len(), 1);
    assert_eq!(registry.workspaces[&name("family")].root, root);
}

#[test]
fn later_workspace_create_preserves_the_existing_default() {
    let fixture = Fixture::new();
    let family = fixture.home.path().join("family");
    let work = fixture.home.path().join("work");
    assert_success(&fixture.run(&["workspace", "create", "--root", path_arg(&family)]));

    let output = fixture.run(&["workspace", "create", "--root", path_arg(&work)]);

    assert_success(&output);
    let registry = fixture.registry();
    assert_eq!(registry.default_workspace, name("family"));
    assert_eq!(registry.workspaces.len(), 2);
    assert_eq!(registry.workspaces[&name("work")].root, work);
}

#[cfg(unix)]
#[test]
fn concurrent_successful_creates_all_survive_the_registry_transaction() {
    const WRITERS: usize = 20;

    let fixture = Fixture::new();
    let initial = fixture.home.path().join("initial");
    assert_success(&fixture.run(&["workspace", "create", "--root", path_arg(&initial)]));
    let release = fixture.current_dir.path().join("release-writers");
    let roots = (0..WRITERS)
        .map(|index| fixture.home.path().join(format!("concurrent-{index}")))
        .collect::<Vec<_>>();
    let mut children = Vec::with_capacity(WRITERS);
    for root in &roots {
        children.push(
            fixture
                .barrier_command(&release, &["workspace", "create", "--root", path_arg(root)])
                .spawn()
                .expect("spawn blocked workspace writer"),
        );
    }

    std::fs::write(&release, b"go").expect("release workspace writers");
    let outputs = children
        .into_iter()
        .map(|child| child.wait_with_output().expect("wait for workspace writer"))
        .collect::<Vec<_>>();
    for output in &outputs {
        assert_success(output);
    }

    let registry = fixture.registry();
    assert_eq!(registry.workspaces.len(), WRITERS + 1);
    for index in 0..WRITERS {
        assert!(
            registry
                .workspaces
                .contains_key(&name(&format!("concurrent-{index}"))),
            "successful concurrent writer {index} was lost"
        );
    }
}

#[cfg(unix)]
#[test]
fn first_create_persistence_failure_preserves_its_new_root_chain_for_manual_cleanup() {
    let fixture = Fixture::new();
    let registry_dir = fixture.registry_path().parent().unwrap().to_path_buf();
    std::fs::create_dir_all(&registry_dir).expect("registry directory");
    std::fs::File::create(registry_dir.join(".env.json.transaction.lock"))
        .expect("zero-length transaction lock database");
    std::fs::write(
        registry_dir.join(".env.json.transaction.lock.owner"),
        std::process::id().to_string(),
    )
    .expect("stable transaction lock owner file");
    let root_parent = fixture.home.path().join("created-only-by-command");
    let root = root_parent.join("nested/family");
    let read_only = ReadOnlyDir::new(&registry_dir);

    let output = fixture.run(&["workspace", "create", "--root", path_arg(&root)]);

    drop(read_only);
    assert_failure_contains(
        &output,
        &[
            "Workspace error:",
            "create temporary workspace registry",
            "automatic cleanup was skipped",
            "deepest first",
        ],
    );
    assert!(!fixture.registry_path().exists());
    assert!(root_parent.is_dir());
    assert!(root.is_dir());
}

#[cfg(unix)]
#[test]
fn later_create_persistence_failure_preserves_registry_bytes_and_new_root_chain() {
    let fixture = Fixture::new();
    let family = fixture.home.path().join("family");
    assert_success(&fixture.run(&["workspace", "create", "--root", path_arg(&family)]));
    let registry_bytes = std::fs::read(fixture.registry_path()).expect("registry bytes");
    let registry_dir = fixture.registry_path().parent().unwrap().to_path_buf();
    let root_parent = fixture.home.path().join("created-only-by-command");
    let root = root_parent.join("nested/work");
    let read_only = ReadOnlyDir::new(&registry_dir);

    let output = fixture.run(&["workspace", "create", "--root", path_arg(&root)]);

    drop(read_only);
    assert_failure_contains(
        &output,
        &[
            "Workspace error:",
            "create temporary workspace registry",
            "automatic cleanup was skipped",
            "deepest first",
        ],
    );
    assert_eq!(
        std::fs::read(fixture.registry_path()).expect("registry bytes after failure"),
        registry_bytes
    );
    assert!(root_parent.is_dir());
    assert!(root.is_dir());
}

#[test]
fn workspace_attach_registers_an_existing_root_without_changing_its_contents() {
    let fixture = Fixture::new();
    let family = fixture.home.path().join("family");
    assert_success(&fixture.run(&["workspace", "create", "--root", path_arg(&family)]));
    let shared = fixture.home.path().join("shared");
    std::fs::create_dir(&shared).expect("existing shared root");
    let sentinel = shared.join("keep.txt");
    std::fs::write(&sentinel, "untouched").expect("sentinel");

    let output = fixture.run(&["workspace", "attach", path_arg(&shared)]);

    assert_success(&output);
    let registry = fixture.registry();
    assert_eq!(registry.workspaces[&name("shared")].root, shared);
    assert_eq!(std::fs::read_to_string(sentinel).unwrap(), "untouched");
}

#[test]
fn alias_rename_and_default_mutations_preserve_the_complete_workspace_record() {
    let fixture = Fixture::new();
    let family = fixture.home.path().join("family");
    let work = fixture.home.path().join("work");
    for root in [&family, &work] {
        assert_success(&fixture.run(&["workspace", "create", "--root", path_arg(root)]));
    }
    let mut registry = fixture.registry();
    let work_record = registry.workspaces.get_mut(&name("work")).unwrap();
    work_record.local_user_id = "person-7".to_owned();
    work_record.receiver_enabled = true;
    work_record
        .env
        .insert("custom".to_owned(), serde_json::json!({"nested": 42}));
    RegistryStore::from_path(fixture.registry_path())
        .replace(&registry)
        .unwrap();
    let original = registry.workspaces[&name("work")].clone();

    assert_success(&fixture.run(&["workspace", "alias", "add", "work", "job"]));
    assert_success(&fixture.run(&["workspace", "rename", "job", "office"]));
    assert_success(&fixture.run(&["workspace", "alias", "remove", "office", "job"]));
    assert_success(&fixture.run(&["workspace", "alias", "add", "office", "workplace"]));
    assert_success(&fixture.run(&["workspace", "default", "workplace"]));

    let registry = fixture.registry();
    let renamed = &registry.workspaces[&name("office")];
    assert_eq!(registry.default_workspace, name("office"));
    assert_eq!(renamed.workspace_id, original.workspace_id);
    assert_eq!(renamed.root, original.root);
    assert_eq!(renamed.local_user_id, original.local_user_id);
    assert_eq!(renamed.receiver_enabled, original.receiver_enabled);
    assert_eq!(renamed.env, original.env);
    assert_eq!(
        renamed.aliases,
        std::iter::once(name("workplace")).collect()
    );
}

#[test]
fn workspace_remove_detaches_an_alias_selected_record_and_leaves_root_contents() {
    let fixture = Fixture::new();
    let family = fixture.home.path().join("family");
    let work = fixture.home.path().join("work");
    for root in [&family, &work] {
        assert_success(&fixture.run(&["workspace", "create", "--root", path_arg(root)]));
    }
    let sentinel = work.join("keep.txt");
    std::fs::write(&sentinel, "never delete me").expect("sentinel");
    assert_success(&fixture.run(&["workspace", "alias", "add", "work", "job"]));

    let output = fixture.run(&["workspace", "remove", "job"]);

    assert_success(&output);
    let registry = fixture.registry();
    assert_eq!(registry.workspaces.len(), 1);
    assert!(!registry.workspaces.contains_key(&name("work")));
    assert_eq!(
        std::fs::read_to_string(sentinel).unwrap(),
        "never delete me"
    );
}

#[test]
fn workspace_list_is_sorted_complete_plain_and_accepts_a_global_alias_selector() {
    let fixture = Fixture::new();
    let zeta = fixture.home.path().join("zeta");
    let alpha = fixture.home.path().join("alpha");
    for root in [&zeta, &alpha] {
        assert_success(&fixture.run(&["workspace", "create", "--root", path_arg(root)]));
    }
    assert_success(&fixture.run(&["workspace", "alias", "add", "alpha", "shared"]));
    assert_success(&fixture.run(&["workspace", "alias", "add", "alpha", "a"]));
    let mut registry = fixture.registry();
    let zeta_record = registry.workspaces.get_mut(&name("zeta")).unwrap();
    zeta_record.local_user_id = "user-z".to_owned();
    zeta_record.receiver_enabled = true;
    RegistryStore::from_path(fixture.registry_path())
        .replace(&registry)
        .unwrap();
    std::fs::create_dir_all(alpha.join(".config")).unwrap();
    std::fs::write(
        alpha.join(".config/config.json"),
        r#"{"access_mode":"read-only"}"#,
    )
    .unwrap();

    let output = fixture.run(&["-b", "A", "workspace", "list"]);

    assert_success(&output);
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 list output");
    assert!(!stdout.contains('\x1b'));
    assert_eq!(
        stdout,
        format!(
            "Workspaces\n\n  alpha\n    root: {}\n    aliases: a, shared\n    local user: setup pending\n    receiver: disabled\n    access mode: read-only\n* zeta (default)\n    root: {}\n    aliases: none\n    local user: user-z\n    receiver: enabled\n    access mode: setup pending\n",
            alpha.display(),
            zeta.display()
        )
    );
}

#[cfg(unix)]
#[test]
fn trailing_workspace_selector_forms_do_not_leak_into_binary_task_arguments() {
    let fixture = Fixture::new();
    let family = fixture.home.path().join("family");
    assert_success(&fixture.run(&["workspace", "create", "--root", path_arg(&family)]));
    let markdown_to_pdf = fixture.current_dir.path().join("markdown-to-pdf");
    fake_markdown_to_pdf(&markdown_to_pdf);
    let mut registry = fixture.registry();
    registry
        .workspaces
        .get_mut(&name("family"))
        .unwrap()
        .env
        .insert(
            "markdown_to_pdf_path".to_owned(),
            serde_json::json!(markdown_to_pdf),
        );
    RegistryStore::from_path(fixture.registry_path())
        .replace(&registry)
        .unwrap();
    let tasks_dir = family.join("tasks");
    std::fs::create_dir_all(&tasks_dir).expect("tasks directory");
    std::fs::write(
        tasks_dir.join("tasks.csv"),
        "task_id,task_name,task_type,status,priority,due_date,hard_deadline,start_date,assignee,see_also,notes,project,energy_level,context,estimated_duration,blocked_by,defer_count,created_date,completed_date,last_touched,linear_issue\n",
    )
    .expect("empty tasks CSV");

    for args in [
        vec!["tasks", "today", "--brain", "family", "--no-tui"],
        vec!["tasks", "today", "-b", "family", "--no-tui"],
        vec!["tasks", "today", "--brain=family", "--no-tui"],
    ] {
        let output = fixture.run(&args);
        assert_success(&output);
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("== Today =="),
            "unexpected stdout for {args:?}: {}",
            String::from_utf8_lossy(&output.stdout)
        );
    }
}

#[test]
fn unknown_global_selector_reports_how_to_discover_valid_selectors() {
    let fixture = Fixture::new();
    let family = fixture.home.path().join("family");
    assert_success(&fixture.run(&["workspace", "create", "--root", path_arg(&family)]));

    let output = fixture.run(&["--brain", "missing", "workspace", "list"]);

    assert_failure_contains(
        &output,
        &[
            "Workspace error:",
            "unknown workspace selector missing",
            "brain workspace list",
        ],
    );
}

#[test]
fn workspace_command_error_prints_its_display_once_without_debug_causes() {
    let fixture = Fixture::new();
    let family = fixture.home.path().join("family");
    assert_success(&fixture.run(&["workspace", "create", "--root", path_arg(&family)]));

    let output = fixture.run(&["--brain", "missing", "workspace", "list"]);

    assert!(!output.status.success(), "command unexpectedly succeeded");
    let stderr = String::from_utf8(output.stderr).expect("UTF-8 workspace error");
    let failure = "unknown workspace selector missing";
    let message = "Workspace error: unknown workspace selector missing; run `brain workspace list` to see available names and aliases\n";
    assert_eq!(stderr, message);
    assert_eq!(stderr.matches(message.trim_end()).count(), 1);
    assert_eq!(stderr.matches(failure).count(), 1);
    assert!(
        !stderr.contains("Caused by:"),
        "unexpected source dump: {stderr:?}"
    );
}

#[test]
fn duplicate_workspace_name_reports_the_unique_name_remedy() {
    let fixture = Fixture::new();
    let family = fixture.home.path().join("family");
    let other = fixture.home.path().join("other");
    assert_success(&fixture.run(&["workspace", "create", "--root", path_arg(&family)]));

    let output = fixture.run(&[
        "workspace",
        "create",
        "--name",
        "family",
        "--root",
        path_arg(&other),
    ]);

    assert_failure_contains(
        &output,
        &[
            "Workspace error:",
            "workspace family already exists",
            "unique canonical name",
        ],
    );
}

#[test]
fn duplicate_workspace_alias_reports_the_unique_selector_remedy() {
    let fixture = Fixture::new();
    let family = fixture.home.path().join("family");
    let work = fixture.home.path().join("work");
    for root in [&family, &work] {
        assert_success(&fixture.run(&["workspace", "create", "--root", path_arg(root)]));
    }

    let output = fixture.run(&["workspace", "alias", "add", "work", "family"]);

    assert_failure_contains(
        &output,
        &[
            "Workspace error:",
            "workspace selector family is not unique",
            "unique canonical name or alias",
        ],
    );
}

#[test]
fn duplicate_alias_on_the_same_workspace_fails_without_changing_registry_bytes() {
    let fixture = Fixture::new();
    let family = fixture.home.path().join("family");
    assert_success(&fixture.run(&["workspace", "create", "--root", path_arg(&family)]));
    assert_success(&fixture.run(&["workspace", "alias", "add", "family", "alt"]));
    let registry_bytes = std::fs::read(fixture.registry_path()).expect("registry bytes");

    let output = fixture.run(&["workspace", "alias", "add", "family", "ALT"]);

    assert_failure_contains(
        &output,
        &[
            "Workspace error:",
            "workspace family already has alias alt",
            "remove the existing alias or choose a different one",
        ],
    );
    assert_eq!(
        std::fs::read(fixture.registry_path()).expect("registry bytes after failure"),
        registry_bytes
    );
}

#[test]
fn overlapping_workspace_root_reports_the_safe_root_remedy_without_creating_it() {
    let fixture = Fixture::new();
    let family = fixture.home.path().join("family");
    assert_success(&fixture.run(&["workspace", "create", "--root", path_arg(&family)]));
    let nested = family.join("nested");

    let output = fixture.run(&["workspace", "create", "--root", path_arg(&nested)]);

    assert_failure_contains(
        &output,
        &[
            "Workspace error:",
            "overlap",
            "outside every registered workspace",
        ],
    );
    assert!(!nested.exists());
}
