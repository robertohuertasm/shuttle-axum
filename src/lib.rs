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

// #[shuttle_service::main]
// async fn axum(#[shuttle_shared_db::Postgres] pool: PgPool) -> shuttle_service::ShuttleAxum {
//     pool.execute(include_str!("../db/schema.sql"))
//         .await
//         .map_err(CustomError::new)?;
//     let router = router(pool).await;
//     let sync_wrapper = SyncWrapper::new(router);
//     tracing::info!("Starting axum server");
//     Ok(sync_wrapper)
// }

async fn router(pool: PgPool) -> Router {
    Router::new()
        .route("/", get(root))
        .route("/txt", get(list_tests).post(create_test))
        .route("/txt/:id", delete(delete_test))
        .with_state(pool)
}

// going without macros
async fn main(pool: PgPool) -> shuttle_service::ShuttleAxum {
    pool.execute(include_str!("../db/schema.sql"))
        .await
        .map_err(CustomError::new)?;
    let router = router(pool).await;
    let sync_wrapper = SyncWrapper::new(router);
    tracing::debug!("Starting axum server");
    Ok(sync_wrapper)
}

async fn __shuttle_wrapper(
    factory: &mut dyn shuttle_service::Factory,
    runtime: &shuttle_service::Runtime,
    logger: shuttle_service::Logger,
) -> Result<Box<dyn shuttle_service::Service>, shuttle_service::Error> {
    use shuttle_service::tracing_subscriber::prelude::*;
    use shuttle_service::ResourceBuilder;
    runtime
        .spawn_blocking(move || {
            let filter_layer =
                shuttle_service::tracing_subscriber::EnvFilter::try_from_default_env()
                    .or_else(|_| shuttle_service::tracing_subscriber::EnvFilter::try_new("DEBUG"))
                    .unwrap();
            shuttle_service::tracing_subscriber::registry()
                .with(filter_layer)
                .with(logger)
                .init();
        })
        .await
        .map_err(|e| {
            if e.is_panic() {
                let mes = e
                    .into_panic()
                    .downcast_ref::<&str>()
                    .map(|x| x.to_string())
                    .unwrap_or_else(|| "panicked setting logger".to_string());
                shuttle_service::Error::BuildPanic(mes)
            } else {
                shuttle_service::Error::Custom(
                    shuttle_service::error::CustomError::new(e).context("failed to set logger"),
                )
            }
        })?;

    let pool = shuttle_shared_db::Postgres::new()
        .build(factory, runtime)
        .await?;
    runtime
        .spawn(async {
            main(pool)
                .await
                .map(|ok| Box::new(ok) as Box<dyn shuttle_service::Service>)
        })
        .await
        .map_err(|e| {
            if e.is_panic() {
                let mes = e
                    .into_panic()
                    .downcast_ref::<&str>()
                    .map(|x| x.to_string())
                    .unwrap_or_else(|| "panicked calling main".to_string());
                shuttle_service::Error::BuildPanic(mes)
            } else {
                shuttle_service::Error::Custom(
                    shuttle_service::error::CustomError::new(e).context("failed to call main"),
                )
            }
        })?
}

fn __binder(
    service: Box<dyn shuttle_service::Service>,
    addr: std::net::SocketAddr,
    runtime: &shuttle_service::Runtime,
) -> shuttle_service::ServeHandle {
    use shuttle_service::Context;
    runtime.spawn(async move {
        service
            .bind(addr)
            .await
            .context("failed to bind service")
            .map_err(Into::into)
    })
}

#[no_mangle]
pub extern "C" fn _create_service() -> *mut shuttle_service::Bootstrapper {
    println!("ENTRY POINT");
    let builder: shuttle_service::StateBuilder<Box<dyn shuttle_service::Service>> =
        |factory, runtime, logger| Box::pin(__shuttle_wrapper(factory, runtime, logger));

    let bootstrapper = shuttle_service::Bootstrapper::new(
        builder,
        __binder,
        shuttle_service::Runtime::new().unwrap(),
    );
    let boxed = Box::new(bootstrapper);
    Box::into_raw(boxed)
}
