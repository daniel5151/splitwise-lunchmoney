pub mod routes;
pub mod state;

use std::net::SocketAddr;
use std::sync::Arc;

use anstream::println;
use axum::Router;
use axum::routing::get;
use lm_common::style::*;
use state::ServerState;
use tower_http::cors::CorsLayer;

use crate::cli::ServeArgs;

pub fn build_app(state: Arc<ServerState>) -> Router {
    let api_router = Router::new()
        .route("/get_current_user", get(routes::get_current_user))
        .route("/get_user/{id}", get(routes::get_user))
        .route("/get_categories", get(routes::get_categories))
        .route("/get_currencies", get(routes::get_currencies))
        .route("/get_groups", get(routes::get_groups))
        .route("/get_group/{id}", get(routes::get_group))
        .route("/get_friends", get(routes::get_friends))
        .route("/get_friend/{id}", get(routes::get_friend))
        .route("/get_expenses", get(routes::get_expenses))
        .route("/get_expense/{id}", get(routes::get_expense))
        .route("/get_comments", get(routes::get_comments))
        .route("/get_notifications", get(routes::get_notifications))
        .with_state(Arc::clone(&state));

    // Mount on both `/api/v3.0` prefix and root so both forms of base URL work
    Router::new()
        .nest("/api/v3.0", api_router.clone())
        .merge(api_router)
        .fallback(routes::fallback)
        .layer(CorsLayer::permissive())
}

pub async fn run(args: ServeArgs) -> anyhow::Result<()> {
    let backup_dir = state::discover_backup_dir(args.dir.as_deref())?;
    let state = Arc::new(ServerState::load_from_dir(&backup_dir)?);

    let user_name = state
        .current_user
        .get("first_name")
        .and_then(|v| v.as_str())
        .map(|first| {
            let last = state
                .current_user
                .get("last_name")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            format!("{first} {last}").trim().to_string()
        })
        .unwrap_or_else(|| "Unknown".to_string());

    let user_id = state
        .current_user
        .get("id")
        .and_then(|v| v.as_u64())
        .map(|id| id.to_string())
        .unwrap_or_else(|| "Unknown".to_string());

    let addr: SocketAddr = format!("{}:{}", args.bind, args.port).parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let local_addr = listener.local_addr()?;

    let bar = "─".repeat(65);

    println! {};
    println! { "{STYLE_HEADER}🌐 Splitwise Mock API Server{STYLE_HEADER:#}" };
    println! { "{STYLE_DIM}{bar}{STYLE_DIM:#}" };
    println! { "  Snapshot Source : {STYLE_INFO}{}{STYLE_INFO:#}", state.backup_path.display() };
    println! { "  Account Owner   : {STYLE_INFO}{user_name}{STYLE_INFO:#} (ID: {user_id})" };
    println! { "  Expenses Loaded : {STYLE_INFO}{}{STYLE_INFO:#}", state.expenses.len() };
    println! { "  Groups Loaded   : {STYLE_INFO}{}{STYLE_INFO:#}", state.groups.len() };
    println! { "  Friends Loaded  : {STYLE_INFO}{}{STYLE_INFO:#}", state.friends.len() };
    println! { "  Users Indexed   : {STYLE_INFO}{}{STYLE_INFO:#}", state.users_by_id.len() };
    println! { "{STYLE_DIM}{bar}{STYLE_DIM:#}" };
    println! { "  🚀 Server listening at: {STYLE_SUCCESS}http://{local_addr}{STYLE_SUCCESS:#}" };
    println! { "  🔗 Splitwise API URL  : {STYLE_SUCCESS}http://{local_addr}/api/v3.0{STYLE_SUCCESS:#}" };
    println! {};

    let app = build_app(state);
    axum::serve(listener, app).await?;
    Ok(())
}
