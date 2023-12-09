pub use sea_orm_migration::prelude::*;

mod m20231208_203518_create_request_table;
mod m20231209_000613_create_archive_rules_table;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20231208_203518_create_request_table::Migration),
            Box::new(m20231209_000613_create_archive_rules_table::Migration),
        ]
    }
}
