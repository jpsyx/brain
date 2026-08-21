include!("support/workspace_registry_migration_support.rs");

include!("workspace_registry_migration_sections/initial_flat_migration.rs");
include!("workspace_registry_migration_sections/migration_idempotency.rs");
include!("workspace_registry_migration_sections/v2_upgrade.rs");
include!("workspace_registry_migration_sections/registry_write_order.rs");
include!("workspace_registry_migration_sections/markdown_path_upgrade.rs");
include!("workspace_registry_migration_sections/receiver_origin_upgrade.rs");
