mod config;
mod race;
mod reporter;

use crate::{
    config::{load_config, setup_logging},
    race::{process_races_from_groups, send_transactions, SendAttempt, SentTransaction},
    reporter::generate_report,
};
use log::{error, info};
use solana_sdk::{signature::Keypair, signer::Signer};
use std::{
    collections::HashMap,
    fs::{File, OpenOptions},
    io::{BufRead, BufReader, Write},
    path::Path,
    sync::Arc,
    time::Duration,
};
use tokio::time::sleep;

// Disclaimer / 免责声明
// English: This example only demonstrates how to use our product. It does not constitute trading
// advice or a trading/execution system. Use at your own risk. We are not responsible for any loss
// caused by using this code.
// 中文：本示例仅演示如何使用我们的产品，不构成任何交易建议或交易/执行系统。使用本代码所造成的任何损失，概不负责。



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
    }?;

    let keypair = Arc::new(keypair);
    info!("Signer public key: {}", keypair.pubkey());

    let http_client = Arc::new(reqwest::Client::new());
    let rpc_client = Arc::new(solana_client::nonblocking::rpc_client::RpcClient::new(
        config.base_rpc_url.clone(),
    ));

    // --- Phase 1: Send Transactions and Log to File ---
    let mut all_sends: Vec<SendAttempt> = Vec::new();
    let sent_tx_file_path = "sent_transactions.jsonl";
    let _ = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(sent_tx_file_path)?;

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
                if !sent_txs.is_empty() {
                    let mut file = OpenOptions::new().create(true).append(true).open(sent_tx_file_path)?;
                    for tx in &sent_txs {
                        let json = serde_json::to_string(tx)?;
                        writeln!(file, "{}", json)?;
                    }
                }
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

    // --- Phase 2: Process Results from File ---
    info!("\n--- All Transactions Sent, Processing Results from File ---");
    let file = File::open(sent_tx_file_path)?;
    let reader = BufReader::new(file);
    let mut races: HashMap<u32, Vec<SentTransaction>> = HashMap::new();
    for line in reader.lines() {
        let line = line?;
        if let Ok(tx) = serde_json::from_str::<SentTransaction>(&line) {
            races.entry(tx.race_number).or_default().push(tx);
        }
    }

    if races.is_empty() {
        error!("No sent transactions found in log file. Cannot determine winners.");
        return Ok(());
    }

    match process_races_from_groups(races, Arc::clone(&rpc_client), config.num_transactions).await {
        Ok((race_winners, draws)) => {
            let mut wins: HashMap<String, u32> = HashMap::new();
            for winner in &race_winners {
                *wins.entry(winner.name.clone()).or_insert(0) += 1;
            }

            info!("\n--- Test Complete ---");
            let accelerator_names: Vec<String> =
                config.accelerators.iter().map(|a| a.name.clone()).collect();
            generate_report(
                &all_sends,
                &wins,
                draws,
                config.num_transactions,
                &accelerator_names,
            );
        }
        Err(e) => {
            error!("Failed to process race results: {}", e);
        }
    }

    Ok(())
}
