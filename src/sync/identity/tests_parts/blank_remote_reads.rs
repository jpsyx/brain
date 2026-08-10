// Setup against a backend whose `rclone cat` of a missing object succeeds.

/// An object store with B2's real `rclone cat` semantics: reading a missing
/// object exits 0 with no output instead of failing. `copyto --immutable`
/// refuses to change bytes that are already there.
struct BlankReadRemote {
    objects: RefCell<BTreeMap<String, Vec<u8>>>,
}

impl BlankReadRemote {
    fn new() -> Self {
        Self {
            objects: RefCell::new(BTreeMap::new()),
        }
    }

    fn key(target: &str) -> String {
        target
            .trim_start_matches("BRAIN:shared/brain")
            .trim_start_matches('/')
            .to_owned()
    }

    fn get(&self, key: &str) -> Option<Vec<u8>> {
        self.objects.borrow().get(key).cloned()
    }

    fn run(&self, args: &[String]) -> RemoteCommandOutput {
        match args.first().map(String::as_str) {
            Some("cat") => {
                let key = Self::key(args.get(1).expect("cat target"));
                self.objects
                    .borrow()
                    .get(&key)
                    .map_or_else(|| output(true, b"", ""), |bytes| output(true, bytes, ""))
            }
            Some("lsf") => {
                let target = args.get(1).expect("lsf target");
                let basenames_only = target.ends_with("workspace-claims");
                let prefix = Self::key(target);
                let listing = self.objects.borrow().keys().fold(
                    String::new(),
                    |mut listing, key| {
                        if let Some(relative) = key.strip_prefix(&prefix) {
                            let relative = relative.trim_start_matches('/');
                            let name = if basenames_only {
                                relative.rsplit('/').next().unwrap_or(relative)
                            } else {
                                relative
                            };
                            writeln!(listing, "{name}").expect("listing write");
                        }
                        listing
                    },
                );
                output(true, listing.as_bytes(), "")
            }
            Some("copyto") => {
                let source = args.get(1).expect("copy source");
                let key = Self::key(args.get(2).expect("copy target"));
                let bytes = std::fs::read(source).expect("copy source bytes");
                let mut objects = self.objects.borrow_mut();
                match objects.get(&key) {
                    Some(existing) if *existing != bytes => {
                        output(false, b"", "Source and destination exist but do not match")
                    }
                    Some(_) => output(true, b"", ""),
                    None => {
                        objects.insert(key, bytes);
                        output(true, b"", "")
                    }
                }
            }
            command => panic!("unexpected remote command: {command:?} {args:?}"),
        }
    }
}

#[test]
fn a_pristine_backend_with_blank_reads_stages_a_claim_then_publishes_on_retry() {
    let root = tempfile::tempdir().unwrap();
    let bytes = manifest_bytes(PERSONAL_ID);
    write_manifest(root.path(), &bytes);
    let store = BlankReadRemote::new();
    let claim = format!(".config/workspace-claims/{PERSONAL_ID}.json");

    let staged = ensure_remote_identity_for_setup_with(
        root.path(),
        workspace_id(PERSONAL_ID),
        &remote(),
        |_| Ok(ManifestlessRemoteAdoption::Refuse),
        |_, args| store.run(args),
    )
    .unwrap_err();

    assert!(staged.to_string().contains("claim staged"), "{staged:#}");
    assert_eq!(store.get(REMOTE_MANIFEST), None);
    assert_eq!(store.get(&claim), Some(bytes.clone()));

    ensure_remote_identity_for_setup_with(
        root.path(),
        workspace_id(PERSONAL_ID),
        &remote(),
        |_| Ok(ManifestlessRemoteAdoption::Refuse),
        |_, args| store.run(args),
    )
    .expect("the retry owns the staged claim and may publish");

    assert_eq!(store.get(REMOTE_MANIFEST), Some(bytes));
}

#[test]
fn a_blank_readback_after_publishing_a_claim_is_a_verification_failure() {
    let root = tempfile::tempdir().unwrap();
    let bytes = manifest_bytes(PERSONAL_ID);
    write_manifest(root.path(), &bytes);
    let mut publications = 0;

    let error = ensure_remote_identity_for_setup_with(
        root.path(),
        workspace_id(PERSONAL_ID),
        &remote(),
        |_| Ok(ManifestlessRemoteAdoption::Refuse),
        |_, args| match args.first().map(String::as_str) {
            // Every read is blank, so the claim publication never lands.
            Some("cat" | "lsf") => output(true, b"", ""),
            Some("copyto") => {
                publications += 1;
                output(false, b"", "quota exceeded")
            }
            command => panic!("unexpected remote command: {command:?}"),
        },
    )
    .unwrap_err();

    let message = error.to_string();
    assert!(message.contains("could not publish or verify"), "{error:#}");
    assert!(message.contains("quota exceeded"), "{error:#}");
    assert!(
        !message.contains("does not match the local manifest"),
        "a missing claim is not a mismatched claim: {error:#}"
    );
    assert_eq!(publications, 1, "canonical identity must not be published");
}
