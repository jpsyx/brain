use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;
use std::sync::{Arc, Condvar, Mutex};

use super::*;
use crate::sync::remote::Remote;

const PERSONAL_ID: &str = "8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b";
const FAMILY_ID: &str = "e806258e-491a-436d-9db4-a5ca9903e0d4";
const INGRESS_ID: &str = "c48b0de2-361d-43aa-8e7d-9a60ba6caf39";

fn workspace_id(raw: &str) -> WorkspaceId {
    WorkspaceId::parse(raw).expect("fixed workspace UUID")
}

fn manifest_bytes(id: &str) -> Vec<u8> {
    format!(
            "{{\n  \"schema_version\": 1,\n  \"workspace_id\": \"{id}\",\n  \"receiver_ingress_id\": \"{INGRESS_ID}\",\n  \"minimum_brain_version\": \"0.1.0\"\n}}\n"
        )
        .into_bytes()
}

fn write_manifest(root: &Path, bytes: &[u8]) {
    let path = WorkspaceManifest::path(root);
    std::fs::create_dir_all(path.parent().expect("manifest parent")).unwrap();
    std::fs::write(path, bytes).unwrap();
}

fn remote() -> Remote {
    Remote {
        env: vec![("RCLONE_CONFIG_BRAIN_TYPE".to_owned(), "b2".to_owned())],
        arg: "BRAIN:shared/brain".to_owned(),
    }
}

fn output(success: bool, stdout: &[u8], stderr: &str) -> RemoteCommandOutput {
    RemoteCommandOutput {
        success,
        stdout: stdout.to_vec(),
        stderr: stderr.to_owned(),
    }
}

#[derive(Default)]
struct RaceRemoteState {
    manifest: Option<Vec<u8>>,
    claims: BTreeMap<String, Vec<u8>>,
    higher_claim_published: bool,
    lower_claim_published: bool,
    manifest_publications: usize,
}

#[derive(Default)]
struct RaceRemote {
    state: Mutex<RaceRemoteState>,
    changed: Condvar,
}

impl RaceRemote {
    fn run(&self, args: &[String]) -> RemoteCommandOutput {
        match args.first().map(String::as_str) {
            Some("cat") => {
                let target = args.get(1).expect("cat target");
                let state = self.state.lock().unwrap();
                // B2 exits 0 with no output when the object is missing, so the
                // concurrency guarantees are proven against the backend Brain
                // actually talks to, not a friendlier one that fails loudly.
                if target.ends_with(REMOTE_MANIFEST) {
                    state
                        .manifest
                        .as_ref()
                        .map_or_else(|| output(true, b"", ""), |bytes| output(true, bytes, ""))
                } else {
                    let name = target.rsplit('/').next().unwrap_or_default();
                    state
                        .claims
                        .get(name)
                        .map_or_else(|| output(true, b"", ""), |bytes| output(true, bytes, ""))
                }
            }
            Some("lsf") => {
                let listing = self.state.lock().unwrap().claims.keys().fold(
                    String::new(),
                    |mut listing, name| {
                        if args
                            .get(1)
                            .is_some_and(|target| target.ends_with("workspace-claims"))
                        {
                            writeln!(listing, "{name}").unwrap();
                        } else {
                            writeln!(listing, ".config/workspace-claims/{name}").unwrap();
                        }
                        listing
                    },
                );
                output(true, listing.as_bytes(), "")
            }
            Some("copyto") => {
                let source = args.get(1).expect("copy source");
                let target = args.get(2).expect("copy target");
                let bytes = std::fs::read(source).unwrap();
                let mut state = self.state.lock().unwrap();
                if target.contains("/.config/workspace-claims/") {
                    let name = target.rsplit('/').next().unwrap().to_owned();
                    if name.starts_with(FAMILY_ID) {
                        state.claims.insert(name, bytes);
                        state.higher_claim_published = true;
                        self.changed.notify_all();
                        state = self
                            .changed
                            .wait_while(state, |state| !state.lower_claim_published)
                            .unwrap();
                    } else {
                        state = self
                            .changed
                            .wait_while(state, |state| !state.higher_claim_published)
                            .unwrap();
                        state.claims.insert(name, bytes);
                        state.lower_claim_published = true;
                        self.changed.notify_all();
                    }
                } else {
                    state.manifest_publications += 1;
                    state.manifest = Some(bytes);
                }
                drop(state);
                output(true, b"", "")
            }
            command => panic!("unexpected remote command: {command:?} {args:?}"),
        }
    }

    fn snapshot(&self) -> (usize, Option<Vec<u8>>) {
        let state = self.state.lock().unwrap();
        (state.manifest_publications, state.manifest.clone())
    }
}

