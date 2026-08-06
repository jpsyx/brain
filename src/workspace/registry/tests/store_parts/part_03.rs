
#[test]
fn read_error_identifies_the_operation_path_and_error_kind() {
    let directory = tempfile::tempdir().expect("tempdir");
    let path = directory.path().join("missing.json");

    let error = RegistryStore::load_from(&path).unwrap_err();
    let display = error.to_string();

    assert!(matches!(
        error,
        RegistryError::Io {
            operation: RegistryOperation::ReadRegistry,
            path: ref error_path,
            related_path: None,
            kind: std::io::ErrorKind::NotFound,
            ..
        } if error_path == &path
    ));
    assert!(display.contains("read workspace registry"));
    assert!(display.contains(path.to_str().unwrap()));
}

#[test]
fn parse_error_identifies_the_operation_and_path() {
    let directory = tempfile::tempdir().expect("tempdir");
    let path = directory.path().join("invalid.json");
    fs::write(&path, b"not json").unwrap();

    let error = RegistryStore::load_from(&path).unwrap_err();
    let display = error.to_string();

    assert!(matches!(
        error,
        RegistryError::Json {
            operation: RegistryOperation::ParseRegistry,
            path: ref error_path,
            ..
        } if error_path == &path
    ));
    assert!(display.contains("parse workspace registry JSON"));
    assert!(display.contains(path.to_str().unwrap()));
}

#[test]
fn generated_temporary_path_uses_the_destination_directory() {
    let destination = Path::new("/machine/config/env.json");
    let temporary = crate::workspace::registry::store::unique_temporary_path(destination);

    assert_eq!(temporary.parent(), destination.parent());
    assert_eq!(
        crate::workspace::registry::store::unique_temporary_path(Path::new("env.json")).parent(),
        Some(Path::new("."))
    );
}

#[test]
fn serialization_is_byte_for_byte_deterministic() {
    let directory = tempfile::tempdir().expect("tempdir");
    let first = directory.path().join("first.json");
    let second = directory.path().join("second.json");
    let registry = registry_with_brain_and_family();

    RegistryStore::save_atomic_to(&first, &registry).unwrap();
    RegistryStore::save_atomic_to(&second, &registry).unwrap();

    assert_eq!(fs::read(first).unwrap(), fs::read(second).unwrap());
}
