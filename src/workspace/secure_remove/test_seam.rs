use std::path::Path;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SecureRemoveTestBoundary {
    OpenBeforeEntryStat,
    EntryIdentityVerifiedBeforeRename,
    QuarantineCreatedBeforeOpen,
    QuarantineRenameBeforeVerification,
    QuarantinePromotedBeforeArtifactVerification,
    QuarantineIdentityVerified,
    QuarantineArtifactUnlinkedBeforeDirectoryRemoval,
    RenameMissingBeforeAbsenceCheck,
}

type SecureRemoveTestHook = Box<dyn FnMut(SecureRemoveTestBoundary, &Path)>;

thread_local! {
    static SECURE_REMOVE_TEST_HOOK: std::cell::RefCell<Option<SecureRemoveTestHook>> =
        const { std::cell::RefCell::new(None) };
    static RECOVERY_NOFOLLOW_CHMOD_UNSUPPORTED: std::cell::Cell<bool> =
        const { std::cell::Cell::new(false) };
}

pub(crate) fn with_secure_remove_test_hook<T>(
    hook: impl FnMut(SecureRemoveTestBoundary, &Path) + 'static,
    operation: impl FnOnce() -> T,
) -> T {
    struct HookGuard;

    impl Drop for HookGuard {
        fn drop(&mut self) {
            SECURE_REMOVE_TEST_HOOK.with(|installed| {
                installed.replace(None);
            });
        }
    }

    SECURE_REMOVE_TEST_HOOK.with(|installed| {
        assert!(installed.replace(Some(Box::new(hook))).is_none());
    });
    let _guard = HookGuard;
    operation()
}

pub(crate) fn with_unsupported_recovery_nofollow_chmod<T>(operation: impl FnOnce() -> T) -> T {
    struct InjectionGuard;

    impl Drop for InjectionGuard {
        fn drop(&mut self) {
            RECOVERY_NOFOLLOW_CHMOD_UNSUPPORTED.set(false);
        }
    }

    RECOVERY_NOFOLLOW_CHMOD_UNSUPPORTED.with(|unsupported| {
        assert!(!unsupported.replace(true));
    });
    let _guard = InjectionGuard;
    operation()
}

pub(super) fn recovery_nofollow_chmod_unsupported() -> bool {
    RECOVERY_NOFOLLOW_CHMOD_UNSUPPORTED.get()
}

pub(crate) fn observe_test_boundary(boundary: SecureRemoveTestBoundary, relative: &Path) {
    SECURE_REMOVE_TEST_HOOK.with(|installed| {
        if let Some(hook) = installed.borrow_mut().as_mut() {
            hook(boundary, relative);
        }
    });
}
