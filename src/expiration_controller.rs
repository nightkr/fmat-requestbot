use std::time::Duration;

use entity::request;
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};
use serenity::CacheAndHttp;
use time::OffsetDateTime;

use crate::archive_request_if_required;

pub async fn run(db: &DatabaseConnection, discord: &CacheAndHttp) {
    loop {
        run_turn(db, discord).await;
        tokio::time::sleep(Duration::from_secs(10)).await;
    }
}

async fn run_turn(db: &DatabaseConnection, discord: &CacheAndHttp) {
    let expiring_requests = request::Entity::find()
        .filter(
            request::Column::ArchivedOn
                .is_null()
                .and(request::Column::ExpiresOn.lt(Some(OffsetDateTime::now_utc()))),
        )
        .all(db)
        .await
        .unwrap();
    for req in expiring_requests {
        if let Err(err) = archive_request_if_required(db, req.id, None, discord).await {
            tracing::error!(error = &err as &dyn std::error::Error, request.id = %req.id, "failed to process request expiration, ignoring...");
        }
    }
}
