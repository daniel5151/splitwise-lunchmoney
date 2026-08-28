# `splitwise-backup`

Standalone snapshot tool to download a comprehensive, raw-fidelity archive of your Splitwise account via the Splitwise API v3.

## Key Features

- **Raw Fidelity & Zero Schema Loss**: Stores verbatim server JSON and binary assets directly.
- **Mock Server Replay**: Records full HTTP exchanges (method, path, query, request headers, status, response headers, body) in `raw_exchanges/` for offline mock server playback.
- **Exhaustive Deep Traversal**: Crawls high-level feeds and traverses individual expenses, comment threads, group records, friend ledgers, user profiles, and media assets.
- **Idempotent & Resumable**: Automatically skips already-downloaded entities and media files on subsequent runs.
- **Cloudflare & Rate-Limit Resilient**: Automatically honors HTTP `Retry-After` cooldown headers and applies exponential backoff.

## Usage

```console
# Automatic resolution from lm_utils.toml or $SPLITWISE_API_KEY
$ cargo run -p splitwise-backup --release

# Custom output directory and explicit API key
$ cargo run -p splitwise-backup --release -- -k "YOUR_API_KEY" -o ~/backups/splitwise-snapshot

# Optional flags
$ cargo run -p splitwise-backup --release -- --skip-media        # Skip downloading receipt images & avatars
$ cargo run -p splitwise-backup --release -- --concurrency 2     # Adjust concurrent workers
$ cargo run -p splitwise-backup --release -- --delay-ms 200      # Inter-request delay throttle
$ cargo run -p splitwise-backup --release -- --preserve-api-key  # Keep raw auth tokens in exchange logs
```

## Snapshot Structure

```text
splitwise-backup-<timestamp>/
├── manifest.json                  # High-level snapshot summary and metadata
├── data/                          # Clean domain JSON files
│   ├── current_user.json
│   ├── currencies.json
│   ├── categories.json
│   ├── groups.json & groups/
│   ├── friends.json & friends/
│   ├── users/
│   ├── expenses.json & expenses/
│   ├── comments/
│   └── notifications.json
├── media/                         # Downloaded receipts, user avatars, group covers
│   ├── manifest.json
│   ├── receipts/
│   ├── avatars/
│   ├── groups/
│   └── categories/
└── raw_exchanges/                 # Exact HTTP request/response pairs for offline mock server replay
```
