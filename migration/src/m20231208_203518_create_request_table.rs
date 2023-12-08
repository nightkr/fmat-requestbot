use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(User::Table)
                    .col(
                        ColumnDef::new(User::Id)
                            .uuid()
                            .not_null()
                            .default(PgFunc::gen_random_uuid())
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(User::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(User::DiscordUserId)
                            .big_unsigned()
                            .not_null()
                            .unique_key(),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(Request::Table)
                    .col(
                        ColumnDef::new(Request::Id)
                            .uuid()
                            .not_null()
                            .default(PgFunc::gen_random_uuid())
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Request::CreatedBy).uuid().not_null())
                    .col(
                        ColumnDef::new(Request::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(Request::DiscordMessageId)
                            .big_unsigned()
                            // We only know the message ID once it has been created...
                            // .not_null()
                            .unique_key(),
                    )
                    .col(ColumnDef::new(Request::Title).string().not_null())
                    .foreign_key(
                        ForeignKeyCreateStatement::new()
                            .from_tbl(Request::Table)
                            .from_col(Request::CreatedBy)
                            .to_tbl(User::Table)
                            .to_col(User::Id),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(Task::Table)
                    .col(
                        ColumnDef::new(Task::Id)
                            .uuid()
                            .not_null()
                            .default(PgFunc::gen_random_uuid())
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Task::Request).uuid().not_null())
                    .col(ColumnDef::new(Task::Weight).integer().not_null())
                    .col(ColumnDef::new(Task::Task).string().not_null())
                    .col(ColumnDef::new(Task::AssignedTo).uuid())
                    .col(ColumnDef::new(Task::StartedAt).timestamp_with_time_zone())
                    .col(ColumnDef::new(Task::CompletedAt).timestamp_with_time_zone())
                    .foreign_key(
                        ForeignKeyCreateStatement::new()
                            .from_tbl(Task::Table)
                            .from_col(Task::Request)
                            .to_tbl(Request::Table)
                            .to_col(Request::Id),
                    )
                    .foreign_key(
                        ForeignKeyCreateStatement::new()
                            .from_tbl(Task::Table)
                            .from_col(Task::AssignedTo)
                            .to_tbl(User::Table)
                            .to_col(User::Id),
                    )
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Task::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Request::Table).to_owned())
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum User {
    Table,
    Id,
    CreatedAt,
    DiscordUserId,
}

#[derive(DeriveIden)]
enum Request {
    Table,
    Id,
    CreatedBy,
    CreatedAt,
    DiscordMessageId,
    Title,
}

#[derive(DeriveIden)]
enum Task {
    Table,
    Id,
    Request,
    Weight,
    Task,
    AssignedTo,
    StartedAt,
    CompletedAt,
}
