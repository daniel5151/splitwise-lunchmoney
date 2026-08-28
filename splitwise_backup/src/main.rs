mod backup;
mod cli;
mod client;
mod exchange;
mod media;

use std::sync::Arc;

use anstream::eprintln;
use clap::Parser;
use cli::Cli;
use client::RawClient;
use lm_common::style::*;

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
    let api_key = cli.resolve_api_key()?;
    let output_dir = cli.resolve_output_dir();
    let exchanges_dir = output_dir.join("raw_exchanges");

    let http = reqwest::Client::builder().use_rustls_tls().build()?;

    let client = Arc::new(RawClient::new(
        http,
        api_key,
        cli.api_url.clone(),
        exchanges_dir,
        cli.concurrency,
        std::time::Duration::from_millis(cli.delay_ms),
        !cli.preserve_api_key,
        cli.verbose,
    ));

    backup::run(
        client,
        &output_dir,
        cli.skip_media,
        cli.concurrency,
        &cli.api_url,
    )
    .await
}
