use std::path::Path;
use std::sync::Arc;

use anstream::println;
use anyhow::Context;
use futures_util::stream::StreamExt;
use futures_util::stream::{self};
use lm_common::style::*;
use serde::Deserialize;
use serde::Serialize;

use crate::client::RawClient;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaManifestEntry {
    pub category: String,
    pub entity_id: Option<u64>,
    pub source_url: String,
    pub saved_as: Option<String>,
    pub status: String,
    pub content_type: Option<String>,
    pub bytes: usize,
    pub error: Option<String>,
}

pub struct MediaDownloadTask {
    pub category: String,
    pub entity_id: Option<u64>,
    pub source_url: String,
    pub subfolder: &'static str,
    pub filename_prefix: String,
}

pub async fn download_all_media(
    client: Arc<RawClient>,
    media_dir: &Path,
    current_user: &serde_json::Value,
    categories: &serde_json::Value,
    groups: &[serde_json::Value],
    users: &[serde_json::Value],
    expenses: &[serde_json::Value],
    notifications: &serde_json::Value,
    concurrency: usize,
) -> anyhow::Result<Vec<MediaManifestEntry>> {
    let receipts_dir = media_dir.join("receipts");
    let avatars_dir = media_dir.join("avatars");
    let groups_dir = media_dir.join("groups");
    let categories_dir = media_dir.join("categories");
    let notifs_dir = media_dir.join("notifications");

    std::fs::create_dir_all(&receipts_dir)?;
    std::fs::create_dir_all(&avatars_dir)?;
    std::fs::create_dir_all(&groups_dir)?;
    std::fs::create_dir_all(&categories_dir)?;
    std::fs::create_dir_all(&notifs_dir)?;

    let mut tasks = Vec::new();

    // 1. Expense receipts
    for exp in expenses {
        let exp_id = exp["id"].as_u64();
        if let Some(receipt) = exp.get("receipt") {
            if let Some(orig) = receipt.get("original").and_then(|v| v.as_str()) {
                if !orig.trim().is_empty() && !is_placeholder_url(orig) {
                    tasks.push(MediaDownloadTask {
                        category: "receipt_original".to_string(),
                        entity_id: exp_id,
                        source_url: orig.to_string(),
                        subfolder: "receipts",
                        filename_prefix: format!("{}_original", exp_id.unwrap_or(0)),
                    });
                }
            }
            if let Some(large) = receipt.get("large").and_then(|v| v.as_str()) {
                if !large.trim().is_empty() && !is_placeholder_url(large) {
                    tasks.push(MediaDownloadTask {
                        category: "receipt_large".to_string(),
                        entity_id: exp_id,
                        source_url: large.to_string(),
                        subfolder: "receipts",
                        filename_prefix: format!("{}_large", exp_id.unwrap_or(0)),
                    });
                }
            }
        }
    }

    // 2. User avatars
    let mut all_users = users.to_vec();
    if let Some(cu) = current_user.get("user") {
        all_users.push(cu.clone());
    }

    for u in &all_users {
        let uid = u["id"].as_u64();
        if let Some(pic) = u.get("picture") {
            for size in ["large", "medium", "small"] {
                if let Some(url) = pic.get(size).and_then(|v| v.as_str()) {
                    if !url.trim().is_empty() && !is_placeholder_url(url) {
                        tasks.push(MediaDownloadTask {
                            category: format!("avatar_{size}"),
                            entity_id: uid,
                            source_url: url.to_string(),
                            subfolder: "avatars",
                            filename_prefix: format!("user_{}_{size}", uid.unwrap_or(0)),
                        });
                    }
                }
            }
        }
    }

    // 3. Group avatars and covers
    for g in groups {
        let gid = g["id"].as_u64();
        if let Some(avatar) = g.get("avatar") {
            for size in ["original", "large", "medium", "small"] {
                if let Some(url) = avatar.get(size).and_then(|v| v.as_str()) {
                    if !url.trim().is_empty() && !is_placeholder_url(url) {
                        tasks.push(MediaDownloadTask {
                            category: format!("group_avatar_{size}"),
                            entity_id: gid,
                            source_url: url.to_string(),
                            subfolder: "groups",
                            filename_prefix: format!("group_{}_avatar_{size}", gid.unwrap_or(0)),
                        });
                    }
                }
            }
        }
        if let Some(cover) = g.get("cover_photo") {
            for size in ["xxlarge", "xlarge"] {
                if let Some(url) = cover.get(size).and_then(|v| v.as_str()) {
                    if !url.trim().is_empty() && !is_placeholder_url(url) {
                        tasks.push(MediaDownloadTask {
                            category: format!("group_cover_{size}"),
                            entity_id: gid,
                            source_url: url.to_string(),
                            subfolder: "groups",
                            filename_prefix: format!("group_{}_cover_{size}", gid.unwrap_or(0)),
                        });
                    }
                }
            }
        }
    }

    // 4. Category icons
    if let Some(cats) = categories.get("categories").and_then(|c| c.as_array()) {
        for parent in cats {
            let pid = parent["id"].as_u64();
            if let Some(icon) = parent.get("icon").and_then(|i| i.as_str()) {
                if !icon.trim().is_empty() && !is_placeholder_url(icon) {
                    tasks.push(MediaDownloadTask {
                        category: "category_icon".to_string(),
                        entity_id: pid,
                        source_url: icon.to_string(),
                        subfolder: "categories",
                        filename_prefix: format!("cat_{}_icon", pid.unwrap_or(0)),
                    });
                }
            }
            if let Some(subcats) = parent.get("subcategories").and_then(|s| s.as_array()) {
                for sub in subcats {
                    let subid = sub["id"].as_u64();
                    if let Some(icon) = sub.get("icon").and_then(|i| i.as_str()) {
                        if !icon.trim().is_empty() && !is_placeholder_url(icon) {
                            tasks.push(MediaDownloadTask {
                                category: "subcategory_icon".to_string(),
                                entity_id: subid,
                                source_url: icon.to_string(),
                                subfolder: "categories",
                                filename_prefix: format!("subcat_{}_icon", subid.unwrap_or(0)),
                            });
                        }
                    }
                }
            }
        }
    }

    // 5. Notification images
    if let Some(notifs) = notifications
        .get("notifications")
        .and_then(|n| n.as_array())
    {
        for n in notifs {
            let nid = n["id"].as_u64();
            if let Some(img) = n.get("image_url").and_then(|i| i.as_str()) {
                if !img.trim().is_empty() && !is_placeholder_url(img) {
                    tasks.push(MediaDownloadTask {
                        category: "notification_image".to_string(),
                        entity_id: nid,
                        source_url: img.to_string(),
                        subfolder: "notifications",
                        filename_prefix: format!("notif_{}", nid.unwrap_or(0)),
                    });
                }
            }
        }
    }

    // Deduplicate tasks by source_url
    let mut seen_urls = std::collections::HashSet::new();
    tasks.retain(|t| seen_urls.insert(t.source_url.clone()));

    let total = tasks.len();
    if total == 0 {
        println! { "  {STYLE_DIM}⏭ No media assets found to download{STYLE_DIM:#}" };
        return Ok(Vec::new());
    }

    println! {
        "  {STYLE_INFO}📥 Downloading {} media assets (receipts, avatars, icons)...{STYLE_INFO:#}",
        total
    };

    let media_dir = media_dir.to_path_buf();
    let stream = stream::iter(tasks)
        .map(|task| {
            let client = Arc::clone(&client);
            let media_dir = media_dir.clone();
            async move {
                let target_subfolder = media_dir.join(task.subfolder);
                let ext = guess_extension(&task.source_url);
                let initial_filename = format!("{}.{}", task.filename_prefix, ext);
                let initial_path = target_subfolder.join(&initial_filename);

                if initial_path.is_file() {
                    if let Ok(meta) = std::fs::metadata(&initial_path) {
                        if meta.len() > 0 {
                            return MediaManifestEntry {
                                category: task.category,
                                entity_id: task.entity_id,
                                source_url: task.source_url,
                                saved_as: Some(format!("{}/{}", task.subfolder, initial_filename)),
                                status: "success".to_string(),
                                content_type: None,
                                bytes: meta.len() as usize,
                                error: None,
                            };
                        }
                    }
                }

                match client.download_media_bytes(&task.source_url).await {
                    Ok((bytes, content_type)) => {
                        let final_ext = if let Some(ct) = &content_type {
                            extension_for_mime(ct).unwrap_or(ext)
                        } else {
                            ext
                        };
                        let final_filename = format!("{}.{}", task.filename_prefix, final_ext);
                        let final_path = target_subfolder.join(&final_filename);

                        if let Err(e) = std::fs::write(&final_path, &bytes) {
                            return MediaManifestEntry {
                                category: task.category,
                                entity_id: task.entity_id,
                                source_url: task.source_url,
                                saved_as: None,
                                status: "error".to_string(),
                                content_type,
                                bytes: 0,
                                error: Some(format!("Failed to save file: {e}")),
                            };
                        }

                        MediaManifestEntry {
                            category: task.category,
                            entity_id: task.entity_id,
                            source_url: task.source_url,
                            saved_as: Some(format!("{}/{}", task.subfolder, final_filename)),
                            status: "success".to_string(),
                            content_type,
                            bytes: bytes.len(),
                            error: None,
                        }
                    }
                    Err(e) => MediaManifestEntry {
                        category: task.category,
                        entity_id: task.entity_id,
                        source_url: task.source_url,
                        saved_as: None,
                        status: "error".to_string(),
                        content_type: None,
                        bytes: 0,
                        error: Some(e.to_string()),
                    },
                }
            }
        })
        .buffer_unordered(concurrency.max(1));

    let entries: Vec<MediaManifestEntry> = stream.collect().await;

    let successful = entries.iter().filter(|e| e.status == "success").count();
    let failed = entries.len() - successful;

    if failed > 0 {
        println! {
            "  {STYLE_WARNING}⚠ Media download complete: {} downloaded, {} failed{STYLE_WARNING:#}",
            successful, failed
        };
    } else {
        println! {
            "  {STYLE_INFO}✓ Downloaded all {} media assets{STYLE_INFO:#}",
            successful
        };
    }

    let manifest_path = media_dir.join("manifest.json");
    let file = std::fs::File::create(&manifest_path).with_context(|| {
        format!(
            "Failed to create media manifest at {}",
            manifest_path.display()
        )
    })?;
    let writer = std::io::BufWriter::new(file);
    serde_json::to_writer_pretty(writer, &entries)?;

    Ok(entries)
}

fn is_placeholder_url(url: &str) -> bool {
    // Check if the URL is an obvious default or nil asset
    url.contains("default-avatar") || url.contains("nil") || url.starts_with("data:")
}

fn guess_extension(url: &str) -> &'static str {
    let clean = url.split('?').next().unwrap_or(url);
    if clean.ends_with(".png") {
        "png"
    } else if clean.ends_with(".jpg") || clean.ends_with(".jpeg") {
        "jpg"
    } else if clean.ends_with(".gif") {
        "gif"
    } else if clean.ends_with(".webp") {
        "webp"
    } else if clean.ends_with(".pdf") {
        "pdf"
    } else if clean.ends_with(".svg") {
        "svg"
    } else {
        "jpg"
    }
}

fn extension_for_mime(mime: &str) -> Option<&'static str> {
    let mime = mime.to_ascii_lowercase();
    if mime.contains("image/png") {
        Some("png")
    } else if mime.contains("image/jpeg") {
        Some("jpg")
    } else if mime.contains("image/gif") {
        Some("gif")
    } else if mime.contains("image/webp") {
        Some("webp")
    } else if mime.contains("image/svg+xml") {
        Some("svg")
    } else if mime.contains("application/pdf") {
        Some("pdf")
    } else {
        None
    }
}
