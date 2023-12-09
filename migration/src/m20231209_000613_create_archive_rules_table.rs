use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(ArchiveRule::Table)
                    .col(
                        ColumnDef::new(ArchiveRule::FromChannel)
                            .big_unsigned()
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(ArchiveRule::ToChannel)
                            .big_unsigned()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(ArchiveRule::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum ArchiveRule {
    Table,
    FromChannel,
    ToChannel,
}
