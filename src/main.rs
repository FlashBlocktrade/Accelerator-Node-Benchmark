mod config;
mod race;
mod reporter;

use crate::{
    config::{load_config, setup_logging},
    race::{process_races, send_transactions, SentTransaction, SendAttempt},
    reporter::generate_report,
};
use log::{error, info};
use solana_sdk::{signature::Keypair, signer::Signer};
use std::{
    collections::HashMap,
    path::Path,
    sync::Arc,
    time::Duration,
};
use tokio::time::sleep;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = load_config()?;
    setup_logging(&config.log_level);

    if config.accelerators.is_empty() {
        error!("Please provide at least one accelerator in config.toml to test.");
        return Err("No accelerators in config".into());
    }

    info!("Starting On-Chain Speed Test...");
    info!("---------------------------------");
    info!("Log Level: {}", config.log_level);
    info!("Base RPC URL: {}", config.base_rpc_url);
    for (i, acc) in config.accelerators.iter().enumerate() {
        info!("Accelerator #{}: {}", i + 1, acc.name);
    }
    info!("Number of tests: {}", config.num_transactions);
    info!("Interval: {}ms", config.interval_ms);
    info!("---------------------------------");

    let keypair = {
        let signer_str = &config.signer;
        let path_str = shellexpand::tilde(signer_str).to_string();
        if Path::new(&path_str).exists() {
            info!("Loading keypair from file: {}", path_str);
            solana_sdk::signature::read_keypair_file(&path_str)
        } else {
            info!("Attempting to decode keypair from base58 string.");
            let bytes = bs58::decode(signer_str).into_vec()?;
            Keypair::from_bytes(&bytes).map_err(|e| e.to_string().into())
        }
    } ?;

    let keypair = Arc::new(keypair);
    info!("Signer public key: {}", keypair.pubkey());

    let http_client = Arc::new(reqwest::Client::new());
    let rpc_client = Arc::new(solana_client::nonblocking::rpc_client::RpcClient::new(
        config.base_rpc_url.clone(),
    ));



    let mut all_sent_txs: Vec<SentTransaction> = Vec::new();
    let mut all_sends: Vec<SendAttempt> = Vec::new();

    for i in 0..config.num_transactions {
        info!("--- Sending Transactions for Race #{} ---", i + 1);
        match send_transactions(
            Arc::clone(&http_client),
            &config.accelerators,
            Arc::clone(&keypair),
            config.tip_amount_sol,
            Arc::clone(&rpc_client),
            i,
        )
        .await
        {
            Ok((sent_txs, send_attempts)) => {
                all_sent_txs.extend(sent_txs);
                all_sends.extend(send_attempts);
            }
            Err(e) => {
                error!("Failed to send transactions for race #{}: {}", i + 1, e);
            }
        }

        if i < config.num_transactions - 1 {
            sleep(Duration::from_millis(config.interval_ms)).await;
        }
    }

    info!("\n--- All Transactions Sent, Processing Results ---");
    match process_races(all_sent_txs, Arc::clone(&rpc_client), config.num_transactions).await {
        Ok((race_winners, draws)) => {
            let mut wins: HashMap<String, u32> = HashMap::new();
            for winner in &race_winners {
                *wins.entry(winner.name.clone()).or_insert(0) += 1;
            }

            info!("\n--- Test Complete ---");
            let accelerator_names: Vec<String> = config.accelerators.iter().map(|a| a.name.clone()).collect();
            generate_report(&all_sends, &wins, draws, config.num_transactions, &accelerator_names);
        }
        Err(e) => {
            error!("Failed to process race results: {}", e);
        }
    }

    Ok(())
}
