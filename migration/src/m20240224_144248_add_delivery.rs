use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Delivery::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Delivery::Id)
                            .uuid()
                            .not_null()
                            .default(PgFunc::gen_random_uuid())
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Delivery::CreatedBy).uuid().not_null())
                    .col(
                        ColumnDef::new(Delivery::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(Delivery::DiscordMessageId)
                            .big_unsigned()
                            .unique_key(),
                    )
                    .foreign_key(
                        ForeignKeyCreateStatement::new()
                            .from_tbl(Delivery::Table)
                            .from_col(Delivery::CreatedBy)
                            .to_tbl(User::Table)
                            .to_col(User::Id),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(DeliveryItem::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(DeliveryItem::Id)
                            .uuid()
                            .not_null()
                            .default(PgFunc::gen_random_uuid())
                            .primary_key(),
                    )
                    .col(ColumnDef::new(DeliveryItem::Delivery).uuid().not_null())
                    .col(ColumnDef::new(DeliveryItem::ItemName).string().not_null())
                    .col(ColumnDef::new(DeliveryItem::Amount).integer().not_null())
                    .foreign_key(
                        ForeignKeyCreateStatement::new()
                            .from_tbl(DeliveryItem::Table)
                            .from_col(DeliveryItem::Delivery)
                            .to_tbl(Delivery::Table)
                            .to_col(Delivery::Id),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(DeliveryItem::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Delivery::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum Delivery {
    Table,
    Id,
    CreatedBy,
    CreatedAt,
    DiscordMessageId,
}

#[derive(DeriveIden)]
enum DeliveryItem {
    Table,
    Id,
    Delivery,
    ItemName,
    Amount,
}

#[derive(DeriveIden)]
enum User {
    Table,
    Id,
}
