use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Instant;

use anstream::println;
use futures_util::stream::StreamExt;
use futures_util::stream::{self};
use lm_common::style::*;
use serde::Serialize;

use crate::client::RawClient;
use crate::client::read_json_file;
use crate::client::write_json_file;
use crate::media::download_all_media;

#[derive(Debug, Serialize)]
pub struct BackupManifest {
    pub timestamp: String,
    pub api_url: String,
    pub user: Option<serde_json::Value>,
    pub stats: BackupStats,
    pub duration_seconds: f64,
}

#[derive(Debug, Serialize, Default)]
pub struct BackupStats {
    pub groups_count: usize,
    pub friends_count: usize,
    pub users_count: usize,
    pub expenses_count: usize,
    pub comments_fetched_count: usize,
    pub notifications_count: usize,
    pub raw_exchanges_count: usize,
    pub media_files_count: usize,
}

pub async fn run(
    client: Arc<RawClient>,
    output_dir: &Path,
    skip_media: bool,
    concurrency: usize,
    api_url: &str,
) -> anyhow::Result<()> {
    let start_time = Instant::now();
    let timestamp_str = jiff::Zoned::now().to_string();

    // Create directory tree
    let raw_exchanges_dir = output_dir.join("raw_exchanges");
    let data_dir = output_dir.join("data");
    let groups_dir = data_dir.join("groups");
    let friends_dir = data_dir.join("friends");
    let users_dir = data_dir.join("users");
    let expenses_dir = data_dir.join("expenses");
    let comments_dir = data_dir.join("comments");
    let media_dir = output_dir.join("media");

    std::fs::create_dir_all(&raw_exchanges_dir)?;
    std::fs::create_dir_all(&data_dir)?;
    std::fs::create_dir_all(&groups_dir)?;
    std::fs::create_dir_all(&friends_dir)?;
    std::fs::create_dir_all(&users_dir)?;
    std::fs::create_dir_all(&expenses_dir)?;
    std::fs::create_dir_all(&comments_dir)?;

    let bar = "─".repeat(65);

    println! {};
    println! { "{STYLE_HEADER}💾 Splitwise Full API Snapshot Backup{STYLE_HEADER:#}" };
    println! { "{STYLE_DIM}{bar}{STYLE_DIM:#}" };
    println! { "  Target Output : {}", output_dir.display() };
    println! { "  API Base URL  : {}", api_url };
    println! { "  Concurrency   : {}", concurrency };
    println! {};

    // ── Phase 1: Bootstrap & Metadata ────────────────────────
    println! { "{STYLE_HEADER}▶ Phase 1/5: Fetching Identity & System Metadata...{STYLE_HEADER:#}" };

    let current_user_file = data_dir.join("current_user.json");
    let current_user = if let Some(cached) = read_json_file(&current_user_file) {
        println! { "  {STYLE_DIM}✓ Loaded current_user.json from previous run{STYLE_DIM:#}" };
        cached
    } else {
        let val = client.get_json("get_current_user", &[]).await?;
        write_json_file(&current_user_file, &val)?;
        val
    };

    let user_name = current_user
        .get("user")
        .map(|u| {
            let first = u.get("first_name").and_then(|v| v.as_str()).unwrap_or("");
            let last = u.get("last_name").and_then(|v| v.as_str()).unwrap_or("");
            format!("{first} {last}").trim().to_string()
        })
        .unwrap_or_else(|| "Unknown".to_string());
    let current_user_id = current_user
        .get("user")
        .and_then(|u| u.get("id"))
        .and_then(|v| v.as_u64());
    println! { "  {STYLE_INFO}✓{STYLE_INFO:#} Authenticated as: {user_name} (ID: {:?})", current_user_id };

    let currencies_file = data_dir.join("currencies.json");
    let currencies = if let Some(cached) = read_json_file(&currencies_file) {
        cached
    } else {
        let val = client.get_json("get_currencies", &[]).await?;
        write_json_file(&currencies_file, &val)?;
        val
    };
    let currency_count = currencies
        .get("currencies")
        .and_then(|c| c.as_array())
        .map_or(0, |a| a.len());
    println! { "  {STYLE_INFO}✓{STYLE_INFO:#} Currencies: {currency_count} currencies" };

    let categories_file = data_dir.join("categories.json");
    let categories = if let Some(cached) = read_json_file(&categories_file) {
        cached
    } else {
        let val = client.get_json("get_categories", &[]).await?;
        write_json_file(&categories_file, &val)?;
        val
    };
    let category_count = categories
        .get("categories")
        .and_then(|c| c.as_array())
        .map_or(0, |a| a.len());
    println! { "  {STYLE_INFO}✓{STYLE_INFO:#} Categories: {category_count} categories" };

    println! {};

    // ── Phase 2: Core Graph Collections ──────────────────────
    println! { "{STYLE_HEADER}▶ Phase 2/5: Fetching Groups, Friends, and Notifications...{STYLE_HEADER:#}" };

    let groups_file = data_dir.join("groups.json");
    let groups_resp = if let Some(cached) = read_json_file(&groups_file) {
        println! { "  {STYLE_DIM}✓ Loaded groups.json from previous run{STYLE_DIM:#}" };
        cached
    } else {
        let val = client.get_json("get_groups", &[]).await?;
        write_json_file(&groups_file, &val)?;
        val
    };
    let groups_list = groups_resp
        .get("groups")
        .and_then(|g| g.as_array())
        .cloned()
        .unwrap_or_default();
    println! { "  {STYLE_INFO}✓{STYLE_INFO:#} Groups: {} groups found", groups_list.len() };

    let friends_file = data_dir.join("friends.json");
    let friends_resp = if let Some(cached) = read_json_file(&friends_file) {
        println! { "  {STYLE_DIM}✓ Loaded friends.json from previous run{STYLE_DIM:#}" };
        cached
    } else {
        let val = client.get_json("get_friends", &[]).await?;
        write_json_file(&friends_file, &val)?;
        val
    };
    let friends_list = friends_resp
        .get("friends")
        .and_then(|f| f.as_array())
        .cloned()
        .unwrap_or_default();
    println! { "  {STYLE_INFO}✓{STYLE_INFO:#} Friends: {} friends found", friends_list.len() };

    let notifs_file = data_dir.join("notifications.json");
    let notifs_resp = if let Some(cached) = read_json_file(&notifs_file) {
        cached
    } else {
        let val = client
            .get_json("get_notifications", &[("limit", "0")])
            .await?;
        write_json_file(&notifs_file, &val)?;
        val
    };
    let notifs_list = notifs_resp
        .get("notifications")
        .and_then(|n| n.as_array())
        .map_or(0, |a| a.len());
    println! { "  {STYLE_INFO}✓{STYLE_INFO:#} Notifications: {} notifications found", notifs_list };

    println! {};

    // ── Phase 3: Paginated Expenses Feed ─────────────────────
    println! { "{STYLE_HEADER}▶ Phase 3/5: Fetching Complete Historical Expenses...{STYLE_HEADER:#}" };
    let expenses_file = data_dir.join("expenses.json");
    let all_expenses: Vec<serde_json::Value> = if let Some(cached) = read_json_file(&expenses_file)
    {
        let arr = cached
            .get("expenses")
            .and_then(|e| e.as_array())
            .cloned()
            .unwrap_or_default();
        println! { "  {STYLE_DIM}✓ Loaded {} expenses from data/expenses.json{STYLE_DIM:#}", arr.len() };
        arr
    } else {
        let (fetched, _) = fetch_all_expenses(&client).await?;
        write_json_file(
            &expenses_file,
            &serde_json::json!({
                "expenses": fetched,
                "total_count": fetched.len()
            }),
        )?;
        fetched
    };
    println! {
        "  {STYLE_INFO}✓{STYLE_INFO:#} All Expenses: {} total transactions aggregated into data/expenses.json",
        all_expenses.len()
    };

    println! {};

    // ── Phase 4: Deep Traversal & Entity Extraction ──────────
    println! { "{STYLE_HEADER}▶ Phase 4/5: Traversing Deep Entity Details (Groups, Friends, Users, Expense details & Comments)...{STYLE_HEADER:#}" };

    // 1. Groups
    let mut group_ids = HashSet::new();
    for g in &groups_list {
        if let Some(id) = g.get("id").and_then(|v| v.as_u64()) {
            if id > 0 {
                group_ids.insert(id);
            }
        }
    }
    for exp in &all_expenses {
        if let Some(gid) = exp.get("group_id").and_then(|v| v.as_u64()) {
            if gid > 0 {
                group_ids.insert(gid);
            }
        }
    }

    let mut detailed_groups = Vec::new();
    let pending_group_ids: Vec<u64> = group_ids
        .into_iter()
        .filter(|gid| {
            let path = groups_dir.join(format!("{gid}.json"));
            if let Some(cached) = read_json_file(&path) {
                if let Some(g) = cached.get("group").cloned() {
                    detailed_groups.push(g);
                }
                false
            } else {
                true
            }
        })
        .collect();

    if !pending_group_ids.is_empty() {
        println! { "  {STYLE_INFO}↳ Fetching {} group detail records...{STYLE_INFO:#}", pending_group_ids.len() };
        let group_stream = stream::iter(pending_group_ids)
            .map(|gid| {
                let client = Arc::clone(&client);
                let groups_dir = groups_dir.clone();
                async move {
                    let endpoint = format!("get_group/{gid}");
                    match client.get_json_optional(&endpoint, &[]).await {
                        Ok(Some(resp)) => {
                            let path = groups_dir.join(format!("{gid}.json"));
                            let _ = write_json_file(&path, &resp);
                            resp.get("group").cloned()
                        }
                        Ok(None) => {
                            // 404 or 403: record marker file so we don't re-query on resume
                            let path = groups_dir.join(format!("{gid}.json"));
                            let _ = write_json_file(&path, &serde_json::json!({"_status": 404, "not_found": true}));
                            None
                        }
                        Err(e) => {
                            eprintln! { "    {STYLE_WARNING}⚠ Failed get_group/{gid}: {e}{STYLE_WARNING:#}" };
                            None
                        }
                    }
                }
            })
            .buffer_unordered(concurrency.max(1));

        let fetched_groups: Vec<Option<serde_json::Value>> = group_stream.collect().await;
        for g in fetched_groups.into_iter().flatten() {
            detailed_groups.push(g);
        }
    }
    println! { "  {STYLE_INFO}✓ Groups detail complete ({} records stored){STYLE_INFO:#}", detailed_groups.len() };

    // 2. Friends
    let mut friend_ids = HashSet::new();
    for f in &friends_list {
        if let Some(id) = f.get("id").and_then(|v| v.as_u64()) {
            friend_ids.insert(id);
        }
    }

    let pending_friend_ids: Vec<u64> = friend_ids
        .into_iter()
        .filter(|fid| !friends_dir.join(format!("{fid}.json")).is_file())
        .collect();

    if !pending_friend_ids.is_empty() {
        println! { "  {STYLE_INFO}↳ Fetching {} friend detail records...{STYLE_INFO:#}", pending_friend_ids.len() };
        let friend_stream = stream::iter(pending_friend_ids)
            .map(|fid| {
                let client = Arc::clone(&client);
                let friends_dir = friends_dir.clone();
                async move {
                    let endpoint = format!("get_friend/{fid}");
                    match client.get_json_optional(&endpoint, &[]).await {
                        Ok(Some(resp)) => {
                            let path = friends_dir.join(format!("{fid}.json"));
                            let _ = write_json_file(&path, &resp);
                        }
                        Ok(None) => {}
                        Err(e) => {
                            eprintln! { "    {STYLE_WARNING}⚠ Failed get_friend/{fid}: {e}{STYLE_WARNING:#}" };
                        }
                    }
                }
            })
            .buffer_unordered(concurrency.max(1));
        friend_stream.collect::<Vec<()>>().await;
    }
    println! { "  {STYLE_INFO}✓ Friends detail complete{STYLE_INFO:#}" };

    // 3. User Profiles
    let mut user_ids = HashSet::new();
    if let Some(cuid) = current_user_id {
        user_ids.insert(cuid);
    }
    for f in &friends_list {
        if let Some(id) = f.get("id").and_then(|v| v.as_u64()) {
            user_ids.insert(id);
        }
    }
    for g in &groups_list {
        if let Some(members) = g.get("members").and_then(|m| m.as_array()) {
            for m in members {
                if let Some(id) = m.get("id").and_then(|v| v.as_u64()) {
                    user_ids.insert(id);
                }
            }
        }
    }
    for exp in &all_expenses {
        if let Some(cb) = exp
            .get("created_by")
            .and_then(|u| u.get("id"))
            .and_then(|v| v.as_u64())
        {
            user_ids.insert(cb);
        }
        if let Some(ub) = exp
            .get("updated_by")
            .and_then(|u| u.get("id"))
            .and_then(|v| v.as_u64())
        {
            user_ids.insert(ub);
        }
        if let Some(db) = exp
            .get("deleted_by")
            .and_then(|u| u.get("id"))
            .and_then(|v| v.as_u64())
        {
            user_ids.insert(db);
        }
        if let Some(users) = exp.get("users").and_then(|u| u.as_array()) {
            for u in users {
                if let Some(uid) = u.get("user_id").and_then(|v| v.as_u64()) {
                    user_ids.insert(uid);
                }
                if let Some(uid) = u
                    .get("user")
                    .and_then(|sub| sub.get("id"))
                    .and_then(|v| v.as_u64())
                {
                    user_ids.insert(uid);
                }
            }
        }
    }

    let mut detailed_users = Vec::new();
    let pending_user_ids: Vec<u64> = user_ids
        .into_iter()
        .filter(|uid| {
            let path = users_dir.join(format!("{uid}.json"));
            if let Some(cached) = read_json_file(&path) {
                if let Some(u) = cached.get("user").cloned() {
                    detailed_users.push(u);
                }
                false
            } else {
                true
            }
        })
        .collect();

    if !pending_user_ids.is_empty() {
        println! { "  {STYLE_INFO}↳ Fetching {} user profile records...{STYLE_INFO:#}", pending_user_ids.len() };
        let user_stream = stream::iter(pending_user_ids)
            .map(|uid| {
                let client = Arc::clone(&client);
                let users_dir = users_dir.clone();
                async move {
                    let endpoint = format!("get_user/{uid}");
                    match client.get_json_optional(&endpoint, &[]).await {
                        Ok(Some(resp)) => {
                            let path = users_dir.join(format!("{uid}.json"));
                            let _ = write_json_file(&path, &resp);
                            resp.get("user").cloned()
                        }
                        Ok(None) => None,
                        Err(e) => {
                            eprintln! { "    {STYLE_WARNING}⚠ Failed get_user/{uid}: {e}{STYLE_WARNING:#}" };
                            None
                        }
                    }
                }
            })
            .buffer_unordered(concurrency.max(1));

        let fetched_users: Vec<Option<serde_json::Value>> = user_stream.collect().await;
        for u in fetched_users.into_iter().flatten() {
            detailed_users.push(u);
        }
    }
    println! { "  {STYLE_INFO}✓ User profiles complete ({} records stored){STYLE_INFO:#}", detailed_users.len() };

    // 4. Expenses & Comments (deep individual fetch)
    let expense_ids: Vec<u64> = all_expenses
        .iter()
        .filter_map(|e| e.get("id").and_then(|v| v.as_u64()))
        .collect();

    let pending_expenses: Vec<u64> = expense_ids
        .into_iter()
        .filter(|eid| {
            let exp_file = expenses_dir.join(format!("{eid}.json"));
            let com_file = comments_dir.join(format!("expense_{eid}.json"));
            !exp_file.is_file() || !com_file.is_file()
        })
        .collect();

    let total_pending = pending_expenses.len();
    if total_pending > 0 {
        println! {
            "  {STYLE_INFO}↳ Fetching individual details & comments for {} expenses (concurrency={})...{STYLE_INFO:#}",
            total_pending, concurrency
        };

        let counter = Arc::new(AtomicUsize::new(0));

        let expense_details_stream = stream::iter(pending_expenses)
            .map(|eid| {
                let client = Arc::clone(&client);
                let expenses_dir = expenses_dir.clone();
                let comments_dir = comments_dir.clone();
                let counter = Arc::clone(&counter);
                async move {
                    let exp_file = expenses_dir.join(format!("{eid}.json"));
                    if !exp_file.is_file() {
                        let exp_endpoint = format!("get_expense/{eid}");
                        match client.get_json_optional(&exp_endpoint, &[]).await {
                            Ok(Some(exp_resp)) => {
                                let _ = write_json_file(&exp_file, &exp_resp);
                            }
                            Ok(None) => {
                                let _ = write_json_file(&exp_file, &serde_json::json!({"_status": 404, "not_found": true}));
                            }
                            Err(_) => {}
                        }
                    }

                    let com_file = comments_dir.join(format!("expense_{eid}.json"));
                    if !com_file.is_file() {
                        let eid_str = eid.to_string();
                        let comments_query = [("expense_id", eid_str.as_str())];
                        match client.get_json_optional("get_comments", &comments_query).await {
                            Ok(Some(com_resp)) => {
                                let _ = write_json_file(&com_file, &com_resp);
                            }
                            Ok(None) => {
                                let _ = write_json_file(&com_file, &serde_json::json!({"_status": 404, "not_found": true}));
                            }
                            Err(_) => {}
                        }
                    }

                    let done = counter.fetch_add(1, Ordering::SeqCst) + 1;
                    if done.is_multiple_of(100) || done == total_pending {
                        println! {
                            "  {STYLE_INFO}↳ Progress: [{done}/{total_pending}] expenses & comments processed ({:.1}%){STYLE_INFO:#}",
                            (done as f64 / total_pending as f64) * 100.0
                        };
                    }
                }
            })
            .buffer_unordered(concurrency.max(1));

        expense_details_stream.collect::<Vec<()>>().await;
    }
    println! { "  {STYLE_INFO}✓ Deep entity traversal completed successfully{STYLE_INFO:#}" };

    println! {};

    // ── Phase 5: Media Downloads ─────────────────────────────
    println! { "{STYLE_HEADER}▶ Phase 5/5: Downloading Media Assets...{STYLE_HEADER:#}" };
    let media_entries = if skip_media {
        println! { "  {STYLE_DIM}⏭ Skipping media downloads (--skip-media){STYLE_DIM:#}" };
        Vec::new()
    } else {
        download_all_media(
            Arc::clone(&client),
            &media_dir,
            &current_user,
            &categories,
            &detailed_groups,
            &detailed_users,
            &all_expenses,
            &notifs_resp,
            concurrency,
        )
        .await?
    };

    println! {};

    // ── Phase 6: Top-Level Manifest ──────────────────────────
    let elapsed = start_time.elapsed().as_secs_f64();
    let stats = BackupStats {
        groups_count: groups_list.len(),
        friends_count: friends_list.len(),
        users_count: detailed_users.len(),
        expenses_count: all_expenses.len(),
        comments_fetched_count: all_expenses.len(),
        notifications_count: notifs_list,
        raw_exchanges_count: client.exchange_count(),
        media_files_count: media_entries
            .iter()
            .filter(|e| e.status == "success")
            .count(),
    };

    let manifest = BackupManifest {
        timestamp: timestamp_str,
        api_url: api_url.to_string(),
        user: current_user.get("user").cloned(),
        stats,
        duration_seconds: elapsed,
    };

    let manifest_path = output_dir.join("manifest.json");
    write_json_file(&manifest_path, &serde_json::to_value(&manifest)?)?;

    println! { "{STYLE_HEADER}🎉 Splitwise Snapshot Completed in {:.2}s{STYLE_HEADER:#}", elapsed };
    println! { "{STYLE_DIM}{bar}{STYLE_DIM:#}" };
    println! { "  Total HTTP Exchanges logged : {STYLE_INFO}{}{STYLE_INFO:#}", client.exchange_count() };
    println! { "  Expenses aggregated         : {STYLE_INFO}{}{STYLE_INFO:#}", all_expenses.len() };
    println! { "  Groups backed up            : {STYLE_INFO}{}{STYLE_INFO:#}", groups_list.len() };
    println! { "  Friends backed up           : {STYLE_INFO}{}{STYLE_INFO:#}", friends_list.len() };
    println! { "  User profiles saved         : {STYLE_INFO}{}{STYLE_INFO:#}", detailed_users.len() };
    println! { "  Media assets saved          : {STYLE_INFO}{}{STYLE_INFO:#}", manifest.stats.media_files_count };
    println! { "  Output location             : {STYLE_HEADER}{}{STYLE_HEADER:#}", output_dir.display() };
    println! {};

    Ok(())
}

/// Paginate through all expenses using limit & offset until exhausted.
async fn fetch_all_expenses(client: &RawClient) -> anyhow::Result<(Vec<serde_json::Value>, usize)> {
    let limit_str = "100";
    let mut offset: usize = 0;
    let mut all_expenses = Vec::new();
    let mut page_count = 0;

    loop {
        page_count += 1;
        let offset_str = offset.to_string();
        let query = [("limit", limit_str), ("offset", offset_str.as_str())];

        let resp = client.get_json("get_expenses", &query).await?;
        let page = resp
            .get("expenses")
            .and_then(|e| e.as_array())
            .cloned()
            .unwrap_or_default();

        let count = page.len();
        all_expenses.extend(page);

        println! {
            "  {STYLE_INFO}↳{STYLE_INFO:#} Page {page_count} (offset={offset}): fetched {count} expenses (cumulative: {})",
            all_expenses.len()
        };

        if count < 100 {
            break;
        }

        offset += count;
    }

    Ok((all_expenses, page_count))
}
