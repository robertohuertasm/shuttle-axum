use std::error::Error;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{delete, get},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use shuttle_service::{error::CustomError, tracing};
use sqlx::{Executor, FromRow, PgPool};
use sync_wrapper::SyncWrapper;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
struct Test {
    id: i32,
    txt: String,
}

type AppError = (StatusCode, String);

fn err<E>(status_code: StatusCode) -> impl FnOnce(E) -> AppError
where
    E: Error,
{
    move |error: E| (status_code, error.to_string())
}

async fn root() -> &'static str {
    "Hello Axum! Shuttle rocks!"
}

async fn create_test(
    State(db): State<PgPool>,
    Json(txt): Json<String>,
) -> Result<Json<Test>, AppError> {
    let test = sqlx::query_as::<_, Test>("INSERT INTO test (txt) VALUES ($1) RETURNING id, txt")
        .bind(txt.as_str())
        .fetch_one(&db)
        .await
        .map_err(err(StatusCode::INTERNAL_SERVER_ERROR))?;

    Ok(Json(test))
}

async fn delete_test(
    State(db): State<PgPool>,
    Path(id): Path<i32>,
) -> Result<Json<Test>, AppError> {
    let test = sqlx::query_as::<_, Test>("DELETE FROM test WHERE id = $1 RETURNING id, txt")
        .bind(id)
        .fetch_one(&db)
        .await
        .map_err(err(StatusCode::INTERNAL_SERVER_ERROR))?;
    Ok(Json(test))
}

async fn list_tests(State(db): State<PgPool>) -> Result<Json<Vec<Test>>, AppError> {
    let tests = sqlx::query_as::<_, Test>("SELECT * FROM test")
        .fetch_all(&db)
        .await
        .map_err(err(StatusCode::INTERNAL_SERVER_ERROR))?;
    Ok(Json(tests))
}

#[shuttle_service::main]
async fn axum(#[shuttle_shared_db::Postgres] pool: PgPool) -> shuttle_service::ShuttleAxum {
    pool.execute(include_str!("../db/schema.sql"))
        .await
        .map_err(CustomError::new)?;
    let router = router(pool).await;
    let sync_wrapper = SyncWrapper::new(router);
    tracing::info!("Starting axum server");
    Ok(sync_wrapper)
}

async fn router(pool: PgPool) -> Router {
    Router::new()
        .route("/", get(root))
        .route("/txt", get(list_tests).post(create_test))
        .route("/txt/:id", delete(delete_test))
        .with_state(pool)
}
