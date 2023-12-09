use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Request::Table)
                    .add_column(ColumnDef::new(Request::ThumbnailUrl).string())
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Request::Table)
                    .drop_column(Request::ThumbnailUrl)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum Request {
    Table,
    ThumbnailUrl,
}
