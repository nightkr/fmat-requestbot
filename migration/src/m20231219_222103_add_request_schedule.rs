use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(RequestSchedule::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(RequestSchedule::Id)
                            .uuid()
                            .not_null()
                            .default(PgFunc::gen_random_uuid())
                            .primary_key(),
                    )
                    .col(ColumnDef::new(RequestSchedule::CreatedBy).uuid().not_null())
                    .col(
                        ColumnDef::new(RequestSchedule::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(ColumnDef::new(RequestSchedule::DisabledAt).timestamp_with_time_zone())
                    .col(
                        ColumnDef::new(RequestSchedule::DiscordMessageId)
                            .big_unsigned()
                            // We only know the message ID once it has been created...
                            // .not_null()
                            .unique_key(),
                    )
                    .col(
                        ColumnDef::new(RequestSchedule::DiscordChannelId)
                            .big_unsigned()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(RequestSchedule::SecondsBetweenRequests)
                            .integer()
                            .not_null(),
                    )
                    .col(ColumnDef::new(RequestSchedule::Title).string().not_null())
                    .col(
                        ColumnDef::new(RequestSchedule::Tasks)
                            .array(ColumnType::String(None))
                            .not_null(),
                    )
                    .col(ColumnDef::new(RequestSchedule::ThumbnailUrl).string())
                    .foreign_key(
                        ForeignKeyCreateStatement::new()
                            .from_tbl(RequestSchedule::Table)
                            .from_col(RequestSchedule::CreatedBy)
                            .to_tbl(User::Table)
                            .to_col(User::Id),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(Request::Table)
                    .add_column(ColumnDef::new(Request::CreatedBySchedule).uuid())
                    .add_foreign_key(
                        &TableForeignKey::new()
                            .name(Request::FkCreatedBySchedule.to_string())
                            .from_tbl(Request::Table)
                            .from_col(Request::CreatedBySchedule)
                            .to_tbl(RequestSchedule::Table)
                            .to_col(RequestSchedule::Id)
                            .to_owned(),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Request::Table)
                    .drop_foreign_key(Request::FkCreatedBySchedule)
                    .drop_column(Request::CreatedBySchedule)
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(Table::drop().table(RequestSchedule::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum RequestSchedule {
    Table,
    Id,
    CreatedBy,
    CreatedAt,
    DisabledAt,
    DiscordMessageId,
    DiscordChannelId,
    SecondsBetweenRequests,
    Title,
    Tasks,
    ThumbnailUrl,
}

#[derive(DeriveIden)]
enum Request {
    Table,
    CreatedBySchedule,
    FkCreatedBySchedule,
}

#[derive(DeriveIden)]
enum User {
    Table,
    Id,
}
