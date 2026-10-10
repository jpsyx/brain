use super::workflow::{Backend, run_with};
use crate::sync::{config::SyncConfig, identity::VerifiedRemote, remote::Remote};
use crate::workspace::{WorkspaceContext, WorkspaceId, WorkspaceName};
use anyhow::{Result, bail};
use std::path::Path;
use std::time::Duration;

#[derive(Default)]
struct TestBackend {
    events: Vec<String>,
    fail: Option<&'static str>,
}
impl Backend for TestBackend {
    fn verify<'a>(
        &mut self,
        _: &WorkspaceContext,
        remote: &'a Remote,
    ) -> Result<VerifiedRemote<'a>> {
        self.events.push("verify".into());
        if self.fail == Some("verify") {
            bail!("wrong workspace");
        }
        Ok(crate::sync::identity::verified_remote_for_test(remote))
    }
    fn signing_endpoint(&mut self, _: &SyncConfig, _: Duration) -> Result<String> {
        self.events.push("private bucket".into());
        Ok("https://s3.us-west-004.backblazeb2.com".into())
    }
    fn upload(&mut self, _: VerifiedRemote<'_>, path: &Path, relative: &str) -> Result<()> {
        assert!(path.is_file());
        self.events.push(format!("upload {relative}"));
        if self.fail == Some("upload") {
            bail!("upload failed");
        }
        Ok(())
    }
    fn sign(&mut self, remote: &Remote, relative: &str, _: Duration) -> Result<String> {
        assert_eq!(remote.arg, "BRAINSHARE:example-bucket/prefix");
        self.events.push(format!("sign {relative}"));
        Ok(format!(
            "https://s3.us-west-004.backblazeb2.com/example-bucket/prefix/{relative}?X-Amz-Algorithm=AWS4-HMAC-SHA256&X-Amz-Date=20261010T120000Z&X-Amz-Expires=3600&X-Amz-Signature=abc"
        ))
    }
}

fn fixture() -> (tempfile::TempDir, WorkspaceContext, SyncConfig) {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("brain");
    std::fs::create_dir_all(root.join("resources/book")).unwrap();
    std::fs::write(root.join("resources/book/chapter.pdf"), b"PDF").unwrap();
    let workspace = WorkspaceContext::new(
        tmp.path(),
        WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b").unwrap(),
        WorkspaceName::parse("example").unwrap(),
        &root,
        "member",
        tmp.path(),
    )
    .unwrap();
    let cfg = serde_json::from_value(serde_json::json!({"enabled":true,"b2_bucket":"example-bucket","b2_path":"prefix","b2_key_id":"example-id","b2_app_key":"example-key"})).unwrap();
    (tmp, workspace, cfg)
}

#[test]
fn sharing_directory_verifies_before_writing_then_uploads_only_zip_and_signs_it() {
    let (_tmp, workspace, cfg) = fixture();
    let mut backend = TestBackend::default();
    let path = workspace.root().join("resources/book");
    let result = run_with(
        &workspace,
        &cfg,
        &path,
        Duration::from_secs(3600),
        &mut backend,
        &mut |_| {},
    )
    .unwrap();
    assert_eq!(
        backend.events,
        [
            "verify",
            "private bucket",
            "upload resources/book/book.zip",
            "sign resources/book/book.zip"
        ]
    );
    assert_eq!(result.path, path.join("book.zip").canonicalize().unwrap());
    assert_eq!(result.expires_at, "2026-10-10T13:00:00+00:00");
}

#[test]
fn failed_identity_never_creates_archive_and_failed_upload_never_signs() {
    let (_tmp, workspace, cfg) = fixture();
    let path = workspace.root().join("resources/book");
    let mut backend = TestBackend {
        fail: Some("verify"),
        ..TestBackend::default()
    };
    assert!(
        run_with(
            &workspace,
            &cfg,
            &path,
            Duration::from_secs(3600),
            &mut backend,
            &mut |_| {}
        )
        .is_err()
    );
    assert!(!path.join("book.zip").exists());
    assert_eq!(backend.events, ["verify"]);
    backend.events.clear();
    backend.fail = Some("upload");
    assert!(
        run_with(
            &workspace,
            &cfg,
            &path.join("chapter.pdf"),
            Duration::from_secs(3600),
            &mut backend,
            &mut |_| {}
        )
        .is_err()
    );
    assert_eq!(
        backend.events,
        [
            "verify",
            "private bucket",
            "upload resources/book/chapter.pdf"
        ]
    );
}

#[test]
fn active_sync_refuses_before_remote_work() {
    let (_tmp, workspace, cfg) = fixture();
    let _guard = crate::sync::lock::try_acquire(&workspace.paths().sync_lock()).unwrap();
    let mut backend = TestBackend::default();
    assert!(
        run_with(
            &workspace,
            &cfg,
            &workspace.root().join("resources/book"),
            Duration::from_secs(3600),
            &mut backend,
            &mut |_| {}
        )
        .is_err()
    );
    assert!(backend.events.is_empty());
}
