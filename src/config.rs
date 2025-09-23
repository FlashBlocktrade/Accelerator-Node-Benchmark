use serde::Deserialize;
use std::collections::HashMap;
use std::{error::Error, fs, str::FromStr};
use log::LevelFilter;

#[derive(Deserialize, Debug)]
pub struct Config {
    pub signer: String,
    pub base_rpc_url: String,
    pub num_transactions: u32,
    pub interval_ms: u64,
    pub log_level: String,
    pub tip_amount_sol: f64,
    pub accelerators: Vec<AcceleratorConfig>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct AcceleratorConfig {
    pub name: String,
    pub url: String,
    pub tip_accounts: Vec<String>,
    pub request_format: RequestFormatConfig,
    #[serde(default)]
    pub headers: HashMap<String, String>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct RequestFormatConfig {
    pub body_template: String,
}

pub fn load_config() -> Result<Config, Box<dyn Error>> {
    let config_str = fs::read_to_string("config.toml")?;
    let config: Config = toml::from_str(&config_str)?;
    Ok(config)
}

pub fn setup_logging(log_level_str: &str) {
    let log_level = LevelFilter::from_str(log_level_str).unwrap_or(LevelFilter::Info);
    env_logger::builder()
        .filter_level(log_level)
        .format_timestamp_micros()
        .init();
}
