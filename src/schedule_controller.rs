use std::time::Duration;

use entity::{request, request_schedule, task};
use migration::{extension::postgres::PgExpr, SimpleExpr};
use sea_orm::{
    sea_query::{self, expr::Expr},
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, Iden, QueryFilter, QuerySelect,
    QueryTrait, Set,
};
use serenity::{http::StatusCode, model::id::ChannelId, CacheAndHttp};
use time::OffsetDateTime;

use crate::render_request;

pub async fn run(db: &DatabaseConnection, discord: &CacheAndHttp) {
    loop {
        run_turn(db, discord).await;
        tokio::time::sleep(Duration::from_secs(10)).await;
    }
}

async fn run_turn(db: &DatabaseConnection, discord: &CacheAndHttp) {
    let triggered_schedulings = request_schedule::Entity::find()
        .filter(request_schedule::Column::DisabledAt.is_null())
        .filter(
            Expr::current_timestamp().gt(Expr::col(
                request_schedule::Column::SecondsBetweenRequests,
            )
            .concat(" seconds")
            .cast_as(CustomType::Interval)
            .add(
                Expr::expr(SimpleExpr::SubQuery(
                    None,
                    Box::new(migration::SubQueryStatement::SelectStatement(
                        request::Entity::find()
                            .select_only()
                            .expr(Expr::col(request::Column::CreatedAt).max())
                            .filter(
                                Expr::col(request::Column::CreatedBySchedule)
                                    .equals(request_schedule::Column::Id),
                            )
                            .into_query(),
                    )),
                ))
                .if_null(OffsetDateTime::UNIX_EPOCH),
            )),
        )
        .all(db)
        .await
        .unwrap();
    for schedule in triggered_schedulings {
        let channel = ChannelId(schedule.discord_channel_id as u64);
        if let Some(discord_message_id) = schedule.discord_message_id {
            if let Err(serenity::Error::Http(err)) =
                channel.message(discord, discord_message_id as u64).await
            {
                if let serenity::prelude::HttpError::UnsuccessfulRequest(req) = &*err {
                    if req.status_code == StatusCode::NOT_FOUND {
                        tracing::info!(
                            message = discord_message_id,
                            schedule_id = %schedule.id,
                            "message could not be found, assuming schedule is deleted"
                        );
                        request_schedule::ActiveModel {
                            disabled_at: Set(Some(OffsetDateTime::now_utc())),
                            ..schedule.into()
                        }
                        .update(db)
                        .await
                        .unwrap();
                        continue;
                    }
                }
            }

            let request = request::ActiveModel {
                title: Set(schedule.title),
                created_by: Set(schedule.created_by),
                discord_channel_id: Set(Some(schedule.discord_channel_id)),
                thumbnail_url: Set(schedule.thumbnail_url),
                created_by_schedule: Set(Some(schedule.id)),
                ..Default::default()
            }
            .insert(db)
            .await
            .unwrap();
            task::Entity::insert_many(schedule.tasks.into_iter().enumerate().map(|(i, task)| {
                task::ActiveModel {
                    request: Set(request.id),
                    weight: Set(i as i32 + 1),
                    task: Set(task),
                    ..Default::default()
                }
            }))
            .exec(db)
            .await
            .unwrap();

            let rendered = render_request(db, request.id).await;
            let message = channel
                .send_message(discord, |msg| rendered.create_message(msg))
                .await
                .unwrap();

            request::ActiveModel {
                discord_message_id: Set(Some(message.id.0 as i64)),
                ..request.into()
            }
            .update(db)
            .await
            .unwrap();
        }
    }
}

#[derive(Iden)]
enum CustomType {
    Interval,
}
