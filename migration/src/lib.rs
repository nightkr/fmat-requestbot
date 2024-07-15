pub use sea_orm_migration::prelude::*;

mod m20231208_203518_create_request_table;
mod m20231209_000613_create_archive_rules_table;
mod m20231209_005830_add_request_channel;
mod m20231209_013836_add_request_thumbnail;
mod m20231219_195822_add_request_archival_flag;
mod m20231219_210033_add_request_expiration_timer;
mod m20240224_144248_add_delivery;
mod m20240715_180531_add_discord_guild;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20231208_203518_create_request_table::Migration),
            Box::new(m20231209_000613_create_archive_rules_table::Migration),
            Box::new(m20231209_005830_add_request_channel::Migration),
            Box::new(m20231209_013836_add_request_thumbnail::Migration),
            Box::new(m20231219_195822_add_request_archival_flag::Migration),
            Box::new(m20231219_210033_add_request_expiration_timer::Migration),
            Box::new(m20240224_144248_add_delivery::Migration),
            Box::new(m20240715_180531_add_discord_guild::Migration),
        ]
    }
}
