use std::sync::Arc;

use anstream::eprintln;
use clap::Parser;
use lm_common::style::*;
use splitwise_backup::backup;
use splitwise_backup::cli::BackupArgs;
use splitwise_backup::cli::Cli;
use splitwise_backup::cli::Commands;
use splitwise_backup::client::RawClient;
use splitwise_backup::server;

#[tokio::main]
async fn main() {
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");

    let cli = Cli::parse();

    if let Err(err) = run(cli).await {
        eprintln! {};
        eprintln! { "{STYLE_ERROR}❌ splitwise_backup error: {err:#}{STYLE_ERROR:#}" };
        eprintln! {};
        std::process::exit(1);
    }
}

async fn run(cli: Cli) -> anyhow::Result<()> {
    match cli.command {
        Some(Commands::Serve(serve_args)) => server::run(serve_args).await,
        Some(Commands::Backup(backup_args)) => run_backup(backup_args).await,
        None => run_backup(cli.backup_args).await,
    }
}

async fn run_backup(args: BackupArgs) -> anyhow::Result<()> {
    let api_key = args.resolve_api_key()?;
    let output_dir = args.resolve_output_dir();
    let exchanges_dir = output_dir.join("raw_exchanges");

    let http = reqwest::Client::builder().use_rustls_tls().build()?;

    let client = Arc::new(RawClient::new(
        http,
        api_key,
        args.api_url.clone(),
        exchanges_dir,
        args.concurrency,
        std::time::Duration::from_millis(args.delay_ms),
        !args.preserve_api_key,
        args.verbose,
    ));

    backup::run(
        client,
        &output_dir,
        args.skip_media,
        args.concurrency,
        &args.api_url,
    )
    .await
}
