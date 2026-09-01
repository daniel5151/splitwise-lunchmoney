use std::str::FromStr;
use std::sync::Arc;

use axum::Json;
use axum::extract::Path;
use axum::extract::Query;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::Deserialize;
use serde_json::Value;

use super::state::ServerState;

#[derive(Debug, Deserialize, Default)]
pub struct GetExpensesParams {
    pub group_id: Option<u64>,
    pub friend_id: Option<u64>,
    pub dated_after: Option<String>,
    pub dated_before: Option<String>,
    pub updated_after: Option<String>,
    pub updated_before: Option<String>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

#[derive(Debug, Deserialize, Default)]
pub struct GetNotificationsParams {
    pub updated_after: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, Default)]
pub struct GetCommentsParams {
    pub expense_id: Option<u64>,
}

pub async fn get_current_user(State(state): State<Arc<ServerState>>) -> impl IntoResponse {
    if state.current_user.is_null() {
        (
            StatusCode::NOT_FOUND,
            Json(
                serde_json::json!({ "errors": { "base": ["Current user not found in snapshot"] } }),
            ),
        )
    } else {
        (
            StatusCode::OK,
            Json(serde_json::json!({ "user": state.current_user })),
        )
    }
}

pub async fn get_user(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<u64>,
) -> impl IntoResponse {
    if let Some(user) = state.users_by_id.get(&id) {
        if is_not_found(user) {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "errors": { "base": ["User not found"] } })),
            );
        }
        if is_forbidden(user) {
            return (
                StatusCode::FORBIDDEN,
                Json(
                    serde_json::json!({ "errors": { "base": ["Access denied to user profile"] } }),
                ),
            );
        }
        (StatusCode::OK, Json(serde_json::json!({ "user": user })))
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "errors": { "base": ["User not found"] } })),
        )
    }
}

pub async fn get_categories(State(state): State<Arc<ServerState>>) -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(serde_json::json!({ "categories": state.categories })),
    )
}

pub async fn get_currencies(State(state): State<Arc<ServerState>>) -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(serde_json::json!({ "currencies": state.currencies })),
    )
}

pub async fn get_groups(State(state): State<Arc<ServerState>>) -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(serde_json::json!({ "groups": state.groups })),
    )
}

pub async fn get_group(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<u64>,
) -> impl IntoResponse {
    if let Some(group) = state.groups_by_id.get(&id) {
        if is_not_found(group) {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "errors": { "base": ["Group not found"] } })),
            );
        }
        (StatusCode::OK, Json(serde_json::json!({ "group": group })))
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "errors": { "base": ["Group not found"] } })),
        )
    }
}

pub async fn get_friends(State(state): State<Arc<ServerState>>) -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(serde_json::json!({ "friends": state.friends })),
    )
}

pub async fn get_friend(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<u64>,
) -> impl IntoResponse {
    if let Some(friend) = state.friends_by_id.get(&id) {
        if is_not_found(friend) {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "errors": { "base": ["Friend not found"] } })),
            );
        }
        (
            StatusCode::OK,
            Json(serde_json::json!({ "friend": friend })),
        )
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "errors": { "base": ["Friend not found"] } })),
        )
    }
}

pub async fn get_expense(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<u64>,
) -> impl IntoResponse {
    if let Some(expense) = state.expenses_by_id.get(&id) {
        if is_not_found(expense) {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "errors": { "base": ["Expense not found"] } })),
            );
        }
        (
            StatusCode::OK,
            Json(serde_json::json!({ "expense": expense })),
        )
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "errors": { "base": ["Expense not found"] } })),
        )
    }
}

pub async fn get_expenses(
    State(state): State<Arc<ServerState>>,
    Query(params): Query<GetExpensesParams>,
) -> impl IntoResponse {
    let dated_after_ts = params
        .dated_after
        .as_deref()
        .and_then(|s| parse_timestamp_filter(s, false));
    let dated_before_ts = params
        .dated_before
        .as_deref()
        .and_then(|s| parse_timestamp_filter(s, true));
    let updated_after_ts = params
        .updated_after
        .as_deref()
        .and_then(|s| parse_timestamp_filter(s, false));
    let updated_before_ts = params
        .updated_before
        .as_deref()
        .and_then(|s| parse_timestamp_filter(s, true));

    let filtered: Vec<Value> = state
        .expenses
        .iter()
        .filter(|exp| {
            if is_not_found(exp) {
                return false;
            }

            // 1. group_id filter
            if let Some(gid) = params.group_id {
                let expense_gid = exp.get("group_id").and_then(|v| v.as_u64()).unwrap_or(0);
                if gid == 0 {
                    if expense_gid != 0 {
                        return false;
                    }
                } else if expense_gid != gid {
                    return false;
                }
            }

            // 2. friend_id filter (only when group_id is not specified or is 0)
            if let Some(fid) = params.friend_id {
                if params.group_id.is_none() || params.group_id == Some(0) {
                    let has_friend =
                        exp.get("users")
                            .and_then(|u| u.as_array())
                            .is_some_and(|users| {
                                users.iter().any(|u| {
                                    u.get("user_id").and_then(|v| v.as_u64()) == Some(fid)
                                        || u.get("user")
                                            .and_then(|sub| sub.get("id"))
                                            .and_then(|v| v.as_u64())
                                            == Some(fid)
                                })
                            });
                    if !has_friend {
                        return false;
                    }
                }
            }

            // 3. dated_after filter
            if let Some(ts_after) = dated_after_ts {
                if let Some(exp_ts) = get_expense_date(exp) {
                    if exp_ts < ts_after {
                        return false;
                    }
                }
            }

            // 4. dated_before filter
            if let Some(ts_before) = dated_before_ts {
                if let Some(exp_ts) = get_expense_date(exp) {
                    if exp_ts > ts_before {
                        return false;
                    }
                }
            }

            // 5. updated_after filter
            if let Some(ts_up_after) = updated_after_ts {
                if let Some(exp_up_ts) = get_expense_updated_at(exp) {
                    if exp_up_ts < ts_up_after {
                        return false;
                    }
                }
            }

            // 6. updated_before filter
            if let Some(ts_up_before) = updated_before_ts {
                if let Some(exp_up_ts) = get_expense_updated_at(exp) {
                    if exp_up_ts > ts_up_before {
                        return false;
                    }
                }
            }

            true
        })
        .cloned()
        .collect();

    let offset = params.offset.unwrap_or(0);
    let limit = params.limit.unwrap_or(0);

    let paged: Vec<Value> = if limit > 0 {
        filtered.into_iter().skip(offset).take(limit).collect()
    } else {
        filtered.into_iter().skip(offset).collect()
    };

    (
        StatusCode::OK,
        Json(serde_json::json!({ "expenses": paged })),
    )
}

pub async fn get_comments(
    State(state): State<Arc<ServerState>>,
    Query(params): Query<GetCommentsParams>,
) -> impl IntoResponse {
    if let Some(eid) = params.expense_id {
        if let Some(comments_doc) = state.comments_by_expense_id.get(&eid) {
            let comments = comments_doc
                .get("comments")
                .cloned()
                .unwrap_or_else(|| serde_json::json!([]));
            return (
                StatusCode::OK,
                Json(serde_json::json!({ "comments": comments })),
            );
        }
    }
    (StatusCode::OK, Json(serde_json::json!({ "comments": [] })))
}

pub async fn get_notifications(
    State(state): State<Arc<ServerState>>,
    Query(params): Query<GetNotificationsParams>,
) -> impl IntoResponse {
    let updated_after_ts = params
        .updated_after
        .as_deref()
        .and_then(|s| parse_timestamp_filter(s, false));

    let filtered: Vec<Value> = state
        .notifications
        .iter()
        .filter(|n| {
            if let Some(ts_after) = updated_after_ts {
                if let Some(created_at_str) = n.get("created_at").and_then(|v| v.as_str()) {
                    if let Ok(notif_ts) = jiff::Timestamp::from_str(created_at_str) {
                        if notif_ts < ts_after {
                            return false;
                        }
                    }
                }
            }
            true
        })
        .cloned()
        .collect();

    let limit = params.limit.unwrap_or(0);
    let paged: Vec<Value> = if limit > 0 {
        filtered.into_iter().take(limit).collect()
    } else {
        filtered
    };

    (
        StatusCode::OK,
        Json(serde_json::json!({ "notifications": paged })),
    )
}

pub async fn fallback() -> impl IntoResponse {
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({
            "error": "Endpoint not found or method not allowed on mock Splitwise API server"
        })),
    )
}

// ── Helpers ──────────────────────────────────────────────────

fn is_not_found(val: &Value) -> bool {
    val.get("not_found").and_then(|v| v.as_bool()) == Some(true)
        || val.get("_status").and_then(|v| v.as_u64()) == Some(404)
}

fn is_forbidden(val: &Value) -> bool {
    val.get("forbidden").and_then(|v| v.as_bool()) == Some(true)
        || val.get("_status").and_then(|v| v.as_u64()) == Some(403)
}

fn parse_timestamp_filter(s: &str, is_end_of_day: bool) -> Option<jiff::Timestamp> {
    let trimmed = s.trim();
    if let Ok(ts) = jiff::Timestamp::from_str(trimmed) {
        return Some(ts);
    }
    if let Ok(date) = jiff::civil::Date::from_str(trimmed) {
        if is_end_of_day {
            return date
                .at(23, 59, 59, 999_999_999)
                .to_zoned(jiff::tz::TimeZone::UTC)
                .ok()
                .map(|z| z.timestamp());
        } else {
            return date
                .at(0, 0, 0, 0)
                .to_zoned(jiff::tz::TimeZone::UTC)
                .ok()
                .map(|z| z.timestamp());
        }
    }
    if let Ok(dt) = jiff::civil::DateTime::from_str(trimmed) {
        return dt
            .to_zoned(jiff::tz::TimeZone::UTC)
            .ok()
            .map(|z| z.timestamp());
    }
    None
}

fn get_expense_date(exp: &Value) -> Option<jiff::Timestamp> {
    exp.get("date")
        .and_then(|v| v.as_str())
        .and_then(|s| parse_timestamp_filter(s, false))
}

fn get_expense_updated_at(exp: &Value) -> Option<jiff::Timestamp> {
    exp.get("updated_at")
        .and_then(|v| v.as_str())
        .and_then(|s| parse_timestamp_filter(s, false))
        .or_else(|| get_expense_date(exp))
}
