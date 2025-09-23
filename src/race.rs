use crate::config::AcceleratorConfig;
use base64::{Engine as _, engine::general_purpose};
use futures::future::join_all;
use log::{debug, error, info, warn};
use rand::Rng;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_client::rpc_config::{RpcBlockConfig, RpcTransactionConfig};
use solana_sdk::{
    commitment_config::CommitmentConfig,

    native_token::sol_to_lamports,
    pubkey::Pubkey,
    signature::{Keypair, Signature, Signer},
    system_instruction,
    transaction::Transaction,
};
use solana_transaction_status::{TransactionDetails, UiTransactionEncoding};
use std::{
    collections::HashMap,
    error::Error,
    str::FromStr,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::time::sleep;

// Result of a single race, contains the winner
#[derive(Debug, Clone)]
pub struct RaceResult {
    pub name: String,
}

// Result of a single API send attempt
#[derive(Debug, Clone)]
pub struct SendAttempt {
    pub name: String,
    pub latency: Duration,
    pub is_success: bool,
}

#[derive(Debug, Clone)]
pub struct SentTransaction {
    pub signature: Signature,
    pub name: String,
    pub race_number: u32,
}

#[derive(Debug, Clone)]
struct ProcessedTransaction {
    signature: Signature,
    name: String,
    race_number: u32,
}

pub async fn send_transactions(
    http_client: Arc<reqwest::Client>,
    accelerators: &[AcceleratorConfig],
    keypair: Arc<Keypair>,
    tip_amount_sol: f64,
    rpc_client: Arc<RpcClient>,
    race_number: u32,
) -> Result<(Vec<SentTransaction>, Vec<SendAttempt>), Box<dyn Error>> {
    let blockhash = rpc_client.get_latest_blockhash().await?;

    let tip_lamports = sol_to_lamports(tip_amount_sol);

    let mut transactions = Vec::new();
    let mut rng = rand::thread_rng();
    for acc_config in accelerators.iter() {
        if acc_config.tip_accounts.is_empty() {
            return Err(format!(
                "No tip_accounts configured for accelerator: {}",
                acc_config.name
            )
            .into());
        }
        let random_index = rng.gen_range(0..acc_config.tip_accounts.len());
        let tip_account_str = &acc_config.tip_accounts[random_index];

        let tip_account = Pubkey::from_str(tip_account_str)?;
        let ix = system_instruction::transfer(&keypair.pubkey(), &tip_account, tip_lamports);
        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&keypair.pubkey()),
            &[&*keypair],
            blockhash,
        );
        transactions.push((tx, acc_config.clone()));
    }



    let mut send_futs = Vec::new();
    for (tx, acc_config) in transactions {
        let encoded_tx = general_purpose::STANDARD.encode(bincode::serialize(&tx)?);
        let http_client = Arc::clone(&http_client);
        let sig = tx.signatures[0];

        send_futs.push(tokio::spawn(async move {
            let send_start = Instant::now();
            let result: Result<reqwest::Response, Box<dyn Error + Send + Sync>> = (async {
                let body_json_str = acc_config
                    .request_format
                    .body_template
                    .replace("{tx}", &encoded_tx);
                let body: serde_json::Value =
                    serde_json::from_str(&body_json_str).map_err(Box::new)?;

                let mut request_builder = http_client.post(&acc_config.url);
                for (key, value) in &acc_config.headers {
                    request_builder = request_builder.header(key, value);
                }

                let res = request_builder.json(&body).send().await.map_err(Box::new)?;
                Ok(res)
            })
            .await;

            let latency = send_start.elapsed();
            (acc_config.name, sig, result, latency)
        }));
    }

    let send_results = join_all(send_futs).await;
    let mut sent_txs = Vec::new();
    let mut send_attempts = Vec::new();

    for join_result in send_results {
        let (name, sig, send_res, latency) = join_result?;
        let mut is_api_success = false;
        match send_res {
            Ok(response) => {
                let status = response.status();
                let text = response.text().await.unwrap_or_else(|e| e.to_string());
                debug!("Response body from {}: {}", name, text);

                if status.is_success() {
                    match serde_json::from_str::<serde_json::Value>(&text) {
                        Ok(json_body) => {
                            if let Some(error_obj) = json_body.get("error") {
                                error!("API Error from {}: {}", name, error_obj.to_string());
                            } else {
                                info!(
                                    "API Success from {}: Latency: {}ms, Signature: {}",
                                    name,
                                    latency.as_millis(),
                                    sig
                                );
                                sent_txs.push(SentTransaction {
                                    signature: sig,
                                    name: name.clone(),
                                    race_number,
                                });
                                is_api_success = true;
                            }
                        }
                        Err(_) => {
                            info!(
                                "API Success from {} (non-JSON): Latency: {}ms",
                                name,
                                latency.as_millis()
                            );
                            sent_txs.push(SentTransaction {
                                signature: sig,
                                name: name.clone(),
                                race_number,
                            });
                            is_api_success = true;
                        }
                    }
                } else {
                    error!("API Error from {}: {}: {}", name, status, text);
                }
            }
            Err(e) => {
                error!("Request Error for {}: {}", name, e);
            }
        }
        send_attempts.push(SendAttempt {
            name,
            latency,
            is_success: is_api_success,
        });
    }

    Ok((sent_txs, send_attempts))
}

pub async fn process_races(
    sent_transactions: Vec<SentTransaction>,
    rpc_client: Arc<RpcClient>,
    num_races: u32,
) -> Result<(Vec<RaceResult>, u32), Box<dyn Error>> {
    if sent_transactions.is_empty() {
        error!("No transactions were successfully sent to any accelerator.");
        return Ok((Vec::new(), num_races));
    }

    info!(
        "Waiting for on-chain confirmation for {} transactions...",
        sent_transactions.len()
    );

    let mut sent_txs_map: HashMap<Signature, SentTransaction> = sent_transactions
        .into_iter()
        .map(|tx| (tx.signature, tx))
        .collect();

    let mut processed_txs: Vec<ProcessedTransaction> = Vec::new();
    let poll_start_time = Instant::now();

    while !sent_txs_map.is_empty() && poll_start_time.elapsed() < Duration::from_secs(180) {
        info!("Polling for {} remaining signatures...", sent_txs_map.len());
        let signatures_to_poll: Vec<Signature> = sent_txs_map.keys().cloned().collect();
        let statuses = rpc_client
            .get_signature_statuses(&signatures_to_poll)
            .await?
            .value;

        let mut landed_sigs = Vec::new();

        for (i, status_opt) in statuses.iter().enumerate() {
            if let Some(status) = status_opt {
                let sig = signatures_to_poll[i];
                if sent_txs_map.contains_key(&sig)
                    && (status.confirmation_status.is_some() || status.err.is_some())
                {
                    let sent_tx = sent_txs_map.remove(&sig).unwrap();
                    landed_sigs.push(sig);

                    let outcome = if let Some(e) = &status.err {
                        format!("Failure({})", e)
                    } else {
                        "Success".to_string()
                    };
                    info!(
                        "Landed on-chain (Status: {}): {} | Signature: {}",
                        outcome, sent_tx.name, sig
                    );

                    processed_txs.push(ProcessedTransaction {
                        signature: sig,
                        name: sent_tx.name,
                        race_number: sent_tx.race_number,
                    });
                }
            }
        }

        if !landed_sigs.is_empty() {
            // Rebuild the map and poll list to only include remaining transactions
            sent_txs_map.retain(|sig, _| !landed_sigs.contains(sig));
        }

        if !sent_txs_map.is_empty() {
            sleep(Duration::from_millis(500)).await;
        }
    }

    if !sent_txs_map.is_empty() {
        warn!(
            "{} transactions did not confirm within the timeout.",
            sent_txs_map.len()
        );
    }

    if processed_txs.is_empty() {
        info!("No transactions landed on-chain within the timeout.");
        return Ok((Vec::new(), num_races));
    }

    let mut race_winners = Vec::new();
    let mut processed_races = 0;

    let mut races: HashMap<u32, Vec<ProcessedTransaction>> = HashMap::new();
    for tx in processed_txs {
        races.entry(tx.race_number).or_default().push(tx);
    }

    for (race_num, race_txs) in races {
        processed_races += 1;
        info!(
            "--- Processing Race #{} ---",
            race_num + 1
        );

        let mut tx_slots = HashMap::new();
        let mut futs = Vec::new();
        for tx in &race_txs {
            let rpc_client_clone = Arc::clone(&rpc_client);
            let sig = tx.signature;
            futs.push(tokio::spawn(async move {
                let config = RpcTransactionConfig {
                    encoding: Some(UiTransactionEncoding::Base64),
                    commitment: Some(CommitmentConfig::confirmed()),
                    max_supported_transaction_version: Some(0),
                };
                match rpc_client_clone
                    .get_transaction_with_config(&sig, config)
                    .await
                {
                    Ok(tx_info) => Some((sig, tx_info.slot)),
                    Err(_) => None,
                }
            }));
        }

        let results = join_all(futs).await;
        let mut earliest_slot = u64::MAX;
        for res in results {
            if let Some(Some((sig, slot))) = res.ok() {
                tx_slots.insert(sig, slot);
                if slot < earliest_slot {
                    earliest_slot = slot;
                }
            }
        }

        for tx in &race_txs {
            if let Some(slot) = tx_slots.get(&tx.signature) {
                info!("Landed transaction: {} in slot {}", tx.name, slot);
            }
        }

        if earliest_slot == u64::MAX {
            error!(
                "Race #{}: Could not determine slot for any processed transaction.",
                race_num + 1
            );
            continue;
        }





        let block_config = RpcBlockConfig {
            encoding: Some(UiTransactionEncoding::Base64),
            transaction_details: Some(TransactionDetails::Full),
            rewards: Some(false),
            commitment: Some(CommitmentConfig::confirmed()),
            max_supported_transaction_version: Some(0),
        };

        let mut block: Option<solana_transaction_status::UiConfirmedBlock> = None;
        const MAX_RETRIES: u32 = 5;

        for i in 0..MAX_RETRIES {
            let block_result = rpc_client
                .get_block_with_config(earliest_slot, block_config.clone())
                .await;
            match block_result {
                Ok(fetched_block) => {
                    block = Some(fetched_block);
                    break;
                }
                Err(e) => {
                    if e.to_string().contains("-32004")
                        || e.to_string().contains("Block not available")
                    {
                        warn!(
                            "Block {} not available, retrying... ({}/{})",
                            earliest_slot,
                            i + 1,
                            MAX_RETRIES
                        );
                        sleep(Duration::from_millis(400)).await;
                    } else {
                        error!("Error fetching block {}: {}", earliest_slot, e);
                        break;
                    }
                }
            }
        }

        let mut winner: Option<&ProcessedTransaction> = None;
        if let Some(block) = block {
            let mut winner_index = u32::MAX;
            let competing_sigs_in_slot: HashMap<Signature, &ProcessedTransaction> = race_txs
                .iter()
                .filter(|tx| tx_slots.get(&tx.signature) == Some(&earliest_slot))
                .map(|tx| (tx.signature, tx))
                .collect();

            if let Some(transactions) = block.transactions {
                for (index, tx_with_meta) in transactions.iter().enumerate() {
                    if let Some(tx) = tx_with_meta.transaction.decode() {
                        if !tx.signatures.is_empty() {
                            let sig = tx.signatures[0];
                            if let Some(competing_tx) = competing_sigs_in_slot.get(&sig) {

                                if (index as u32) < winner_index {
                                    winner_index = index as u32;
                                    winner = Some(competing_tx);
                                }
                            }
                        }
                    }
                }
            }

            if let Some(the_winner) = winner {
                info!(
                    "Winner for Race #{}: {} (Slot {}, Index {})",
                    race_num + 1,
                    the_winner.name,
                    earliest_slot,
                    winner_index
                );
                race_winners.push(RaceResult {
                    name: the_winner.name.clone(),
                });
            } else {
                error!(
                    "Could not find any competing transactions in block {}.",
                    earliest_slot
                );
            }
        } else {
            error!(
                "Could not fetch block {} after retries. Winner for race #{} cannot be determined.",
                earliest_slot,
                race_num + 1
            );
        }
    }

    let draws = num_races - processed_races;
    Ok((race_winners, draws))
}
