use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;

use serde_json::Value;

use crate::client::read_json_file;

/// In-memory store holding the parsed Splitwise API snapshot data.
#[derive(Debug, Clone)]
pub struct ServerState {
    pub current_user: Value,
    pub categories: Value,
    pub currencies: Value,
    pub groups: Vec<Value>,
    pub groups_by_id: HashMap<u64, Value>,
    pub friends: Vec<Value>,
    pub friends_by_id: HashMap<u64, Value>,
    pub users_by_id: HashMap<u64, Value>,
    pub expenses: Vec<Value>,
    pub expenses_by_id: HashMap<u64, Value>,
    pub comments_by_expense_id: HashMap<u64, Value>,
    pub notifications: Vec<Value>,
    pub backup_path: PathBuf,
}

impl ServerState {
    /// Load snapshot state from the specified backup directory.
    pub fn load_from_dir(backup_dir: &Path) -> anyhow::Result<Self> {
        let data_dir = if backup_dir.join("data").is_dir() {
            backup_dir.join("data")
        } else {
            backup_dir.to_path_buf()
        };

        if !data_dir.exists() {
            anyhow::bail!(
                "Backup directory does not exist or is missing data: {}",
                backup_dir.display()
            );
        }

        // 1. Current user
        let current_user_file = data_dir.join("current_user.json");
        let current_user_doc = read_json_file(&current_user_file)
            .or_else(|| {
                let manifest_file = backup_dir.join("manifest.json");
                read_json_file(&manifest_file)
                    .and_then(|m| m.get("user").map(|u| serde_json::json!({ "user": u })))
            })
            .unwrap_or_else(|| serde_json::json!({ "user": null }));
        let current_user = current_user_doc.get("user").cloned().unwrap_or(Value::Null);

        // 2. Categories
        let categories_file = data_dir.join("categories.json");
        let categories_doc = read_json_file(&categories_file)
            .unwrap_or_else(|| serde_json::json!({ "categories": [] }));
        let categories = categories_doc
            .get("categories")
            .cloned()
            .unwrap_or_else(|| serde_json::json!([]));

        // 3. Currencies
        let currencies_file = data_dir.join("currencies.json");
        let currencies_doc = read_json_file(&currencies_file)
            .unwrap_or_else(|| serde_json::json!({ "currencies": [] }));
        let currencies = currencies_doc
            .get("currencies")
            .cloned()
            .unwrap_or_else(|| serde_json::json!([]));

        // 4. Groups
        let mut groups = Vec::new();
        let mut groups_by_id = HashMap::new();

        let groups_file = data_dir.join("groups.json");
        if let Some(doc) = read_json_file(&groups_file) {
            if let Some(arr) = doc.get("groups").and_then(|g| g.as_array()) {
                for g in arr {
                    if let Some(id) = g.get("id").and_then(|v| v.as_u64()) {
                        groups_by_id.insert(id, g.clone());
                    }
                    groups.push(g.clone());
                }
            }
        }

        let individual_groups_dir = data_dir.join("groups");
        if individual_groups_dir.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&individual_groups_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("json") {
                        if let Some(doc) = read_json_file(&path) {
                            if let Some(g) = doc.get("group") {
                                if let Some(id) = g.get("id").and_then(|v| v.as_u64()) {
                                    if !groups_by_id.contains_key(&id) {
                                        groups.push(g.clone());
                                    }
                                    groups_by_id.insert(id, g.clone());
                                }
                            }
                        }
                    }
                }
            }
        }

        // 5. Friends
        let mut friends = Vec::new();
        let mut friends_by_id = HashMap::new();

        let friends_file = data_dir.join("friends.json");
        if let Some(doc) = read_json_file(&friends_file) {
            if let Some(arr) = doc.get("friends").and_then(|f| f.as_array()) {
                for f in arr {
                    if let Some(id) = f.get("id").and_then(|v| v.as_u64()) {
                        friends_by_id.insert(id, f.clone());
                    }
                    friends.push(f.clone());
                }
            }
        }

        let individual_friends_dir = data_dir.join("friends");
        if individual_friends_dir.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&individual_friends_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("json") {
                        if let Some(doc) = read_json_file(&path) {
                            if let Some(f) = doc.get("friend") {
                                if let Some(id) = f.get("id").and_then(|v| v.as_u64()) {
                                    if !friends_by_id.contains_key(&id) {
                                        friends.push(f.clone());
                                    }
                                    friends_by_id.insert(id, f.clone());
                                }
                            }
                        }
                    }
                }
            }
        }

        // 6. Expenses
        let mut expenses = Vec::new();
        let mut expenses_by_id = HashMap::new();

        let expenses_file = data_dir.join("expenses.json");
        if let Some(doc) = read_json_file(&expenses_file) {
            if let Some(arr) = doc.get("expenses").and_then(|e| e.as_array()) {
                for exp in arr {
                    if let Some(id) = exp.get("id").and_then(|v| v.as_u64()) {
                        expenses_by_id.insert(id, exp.clone());
                    }
                    expenses.push(exp.clone());
                }
            }
        }

        let individual_expenses_dir = data_dir.join("expenses");
        if individual_expenses_dir.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&individual_expenses_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("json") {
                        if let Some(doc) = read_json_file(&path) {
                            if let Some(exp) = doc.get("expense") {
                                if let Some(id) = exp.get("id").and_then(|v| v.as_u64()) {
                                    if !expenses_by_id.contains_key(&id) {
                                        expenses.push(exp.clone());
                                    }
                                    expenses_by_id.insert(id, exp.clone());
                                }
                            }
                        }
                    }
                }
            }
        }

        // Sort expenses chronologically descending by date
        expenses.sort_by(|a, b| {
            let date_a = a.get("date").and_then(|v| v.as_str()).unwrap_or("");
            let date_b = b.get("date").and_then(|v| v.as_str()).unwrap_or("");
            date_b.cmp(date_a)
        });

        // 7. Users
        let mut users_by_id = HashMap::new();

        // Harvest from current_user
        if let Some(id) = current_user.get("id").and_then(|v| v.as_u64()) {
            users_by_id.insert(id, current_user.clone());
        }

        // Harvest from friends
        for f in &friends {
            if let Some(id) = f.get("id").and_then(|v| v.as_u64()) {
                users_by_id.insert(id, f.clone());
            }
        }

        // Harvest from group members
        for g in &groups {
            if let Some(members) = g.get("members").and_then(|m| m.as_array()) {
                for m in members {
                    if let Some(id) = m.get("id").and_then(|v| v.as_u64()) {
                        users_by_id.entry(id).or_insert_with(|| m.clone());
                    }
                }
            }
        }

        // Harvest from individual user files in users/
        let users_dir = data_dir.join("users");
        if users_dir.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&users_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("json") {
                        if let Some(doc) = read_json_file(&path) {
                            if let Some(u) = doc.get("user") {
                                if let Some(id) = u.get("id").and_then(|v| v.as_u64()) {
                                    users_by_id.insert(id, u.clone());
                                }
                            }
                        }
                    }
                }
            }
        }

        // 8. Comments
        let mut comments_by_expense_id = HashMap::new();
        let comments_dir = data_dir.join("comments");
        if comments_dir.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&comments_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("json") {
                        let stem = path
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or_default();
                        if let Some(id_str) = stem.strip_prefix("expense_") {
                            if let Ok(eid) = id_str.parse::<u64>() {
                                if let Some(doc) = read_json_file(&path) {
                                    comments_by_expense_id.insert(eid, doc);
                                }
                            }
                        }
                    }
                }
            }
        }

        // 9. Notifications
        let notifs_file = data_dir.join("notifications.json");
        let notifications = read_json_file(&notifs_file)
            .and_then(|doc| doc.get("notifications").and_then(|n| n.as_array()).cloned())
            .unwrap_or_default();

        Ok(Self {
            current_user,
            categories,
            currencies,
            groups,
            groups_by_id,
            friends,
            friends_by_id,
            users_by_id,
            expenses,
            expenses_by_id,
            comments_by_expense_id,
            notifications,
            backup_path: backup_dir.to_path_buf(),
        })
    }
}

/// Auto-discover the latest Splitwise backup directory.
pub fn discover_backup_dir(explicit: Option<&Path>) -> anyhow::Result<PathBuf> {
    if let Some(p) = explicit {
        if p.is_dir() {
            return Ok(p.to_path_buf());
        }
        anyhow::bail!(
            "Specified backup directory '{}' does not exist.",
            p.display()
        );
    }

    // Search current directory for splitwise-backup-* directories
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(".") {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if name.starts_with("splitwise-backup-") {
                        candidates.push(path);
                    }
                }
            }
        }
    }

    if let Some(best) = candidates.into_iter().max() {
        return Ok(best);
    }

    anyhow::bail!(
        "No Splitwise backup snapshot directory found in current working directory. \
         Please provide a backup directory via --dir <PATH> or create one with splitwise_backup."
    );
}
