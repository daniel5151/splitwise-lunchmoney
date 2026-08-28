use std::path::Path;
use std::path::PathBuf;

use clap::Parser;

/// Standalone CLI tool to download a comprehensive snapshot of Splitwise account data.
#[derive(Parser, Debug, Clone)]
#[command(
    name = "splitwise_backup",
    about = "Comprehensive, raw-fidelity backup tool for Splitwise API v3",
    version
)]
pub struct Cli {
    /// Splitwise API key (Bearer token). If not specified, checks $SPLITWISE_API_KEY
    /// or lm_utils.toml.
    #[arg(short = 'k', long, env = "SPLITWISE_API_KEY")]
    pub api_key: Option<String>,

    /// Path to lm_utils.toml configuration file
    #[arg(short = 'c', long)]
    pub config: Option<PathBuf>,

    /// Output directory for backup snapshot (default: ./splitwise-backup-<timestamp>)
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// Splitwise API base URL
    #[arg(long, default_value = "https://secure.splitwise.com/api/v3.0")]
    pub api_url: String,

    /// Number of concurrent requests for fetching deep entity details and media
    #[arg(long, default_value_t = 3)]
    pub concurrency: usize,

    /// Delay in milliseconds between consecutive requests from a worker (default: 75ms)
    #[arg(long, default_value_t = 75)]
    pub delay_ms: u64,

    /// Skip downloading media files (receipt attachments, user avatars, group covers)
    #[arg(long)]
    pub skip_media: bool,

    /// Preserve raw API key in recorded HTTP exchange headers (default: redact Authorization header)
    #[arg(long)]
    pub preserve_api_key: bool,

    /// Verbose output logging for each HTTP exchange
    #[arg(short, long)]
    pub verbose: bool,
}

impl Cli {
    /// Resolve the Splitwise API key from CLI flag, environment variable, or lm_utils.toml.
    pub fn resolve_api_key(&self) -> anyhow::Result<String> {
        if let Some(key) = &self.api_key {
            let trimmed = key.trim();
            if !trimmed.is_empty() {
                return Ok(trimmed.to_string());
            }
        }

        // Fallback: Check if lm_utils config file exists and contains [splitwise].api_key
        if let Some((key, path)) = try_load_key_from_config(self.config.as_deref()) {
            if self.verbose {
                anstream::eprintln!("Loaded Splitwise API key from {}", path.display());
            }
            return Ok(key);
        }

        anyhow::bail!(
            "Missing Splitwise API key. Provide it via --api-key <KEY>, the SPLITWISE_API_KEY \
             environment variable, or in your lm_utils.toml configuration file."
        );
    }

    /// Resolve the output directory.
    pub fn resolve_output_dir(&self) -> PathBuf {
        if let Some(dir) = &self.output {
            dir.clone()
        } else {
            let now = jiff::Zoned::now();
            let stamp = now.strftime("%Y-%m-%dT%H-%M-%S").to_string();
            PathBuf::from(format!("splitwise-backup-{}", stamp))
        }
    }
}

/// Helper to try loading the splitwise API key from standard config file locations.
fn try_load_key_from_config(user_path: Option<&Path>) -> Option<(String, PathBuf)> {
    let mut candidates: Vec<PathBuf> = Vec::new();

    if let Some(p) = user_path {
        candidates.push(p.to_path_buf());
    }

    // 1. Current working directory
    candidates.push(PathBuf::from("lm_utils.toml"));

    // 2. Search parent directories
    if let Ok(mut cwd) = std::env::current_dir() {
        candidates.push(cwd.join("lm_utils.toml"));
        while cwd.pop() {
            candidates.push(cwd.join("lm_utils.toml"));
        }
    }

    // 3. Executable directory
    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            candidates.push(exe_dir.join("lm_utils.toml"));
        }
    }

    // 4. User home directories
    if let Ok(home) = std::env::var("HOME") {
        let home_path = PathBuf::from(home);
        candidates.push(home_path.join(".config/lm_utils/lm_utils.toml"));
        candidates.push(home_path.join(".config/lm_utils.toml"));
        candidates.push(home_path.join("lm_utils.toml"));
    }

    for path in candidates {
        if path.is_file() {
            if let Ok(contents) = std::fs::read_to_string(&path) {
                if let Ok(doc) = contents.parse::<toml_edit::DocumentMut>() {
                    if let Some(splitwise_item) = doc.get("splitwise") {
                        if let Some(api_key_item) = splitwise_item.get("api_key") {
                            if let Some(key_str) = api_key_item.as_str() {
                                let trimmed = key_str.trim();
                                if !trimmed.is_empty() {
                                    return Some((trimmed.to_string(), path));
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    None
}
