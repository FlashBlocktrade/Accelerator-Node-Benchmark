# Nodes Speed Test

// Disclaimer / 免责声明
// English: This example only demonstrates how to use our product. It does not constitute trading
// advice or a trading/execution system. Use at your own risk. We are not responsible for any loss
// caused by using this code.
// 中文：本示例仅演示如何使用我们的产品，不构成任何交易建议或交易/执行系统。使用本代码所造成的任何损失，概不负责。

A Rust-based tool for benchmarking the performance of Solana RPC nodes by racing transactions against each other.

## Overview

This tool sends identical transactions to multiple Solana RPC endpoints (accelerators) simultaneously and determines which node gets the transaction confirmed on-chain first. It provides a detailed report of the results, allowing you to compare the speed and reliability of different nodes.

## How it Works

1.  **Configuration**: The tool reads a `config.toml` file which defines the base RPC node, the accelerators to test, and other parameters.
2.  **Transaction Creation**: For each race, a simple tip transaction is created and signed.
3.  **Racing**: The same transaction is sent concurrently to all configured accelerator endpoints.
4.  **Confirmation**: The tool polls the blockchain to see which transaction is the first to be confirmed.
5.  **Reporting**: After all races are complete, a summary report is generated, showing the number of wins, success/failure rates, and latency statistics for each accelerator.

## Configuration

Create a `config.toml` file in the root of the project with the following structure:

```toml
# Path to your signer keypair file or a base58 encoded private key.
signer = "~/.config/solana/id.json"

# A reliable base RPC URL for fetching blockhashes and confirming transactions.
base_rpc_url = "https://api.mainnet-beta.solana.com"

# Number of races to run.
num_transactions = 10

# Interval in milliseconds between each race.
interval_ms = 1000

# Log level: "info", "debug", "warn", "error".
log_level = "info"

# Amount of SOL to send as a tip in each transaction.
tip_amount_sol = 0.0001

# A list of accelerator nodes to test.
[[accelerators]]
name = "Accelerator 1"
url = "https://accelerator1.com/rpc"
tip_accounts = ["<tip_account_pubkey_1>", "<tip_account_pubkey_2>"]
[accelerators.request_format]
body_template = '''
{
    "jsonrpc": "2.0",
    "id": 1,
    "method": "sendTransaction",
    "params": ["{tx}"]
}
'''
[accelerators.headers]
Content-Type = "application/json"

[[accelerators]]
name = "Accelerator 2"
url = "https://accelerator2.com/rpc"
tip_accounts = ["<tip_account_pubkey_3>"]
[accelerators.request_format]
body_template = '''
{
    "jsonrpc": "2.0",
    "id": 1,
    "method": "sendTransaction",
    "params": ["{tx}"]
}
'''
[accelerators.headers]
Authorization = "Bearer your-auth-token"
```

## Usage

1.  **Install Rust**: If you don't have it, install Rust from [https://rustup.rs/](https://rustup.rs/).
2.  **Clone the repository**:
    ```bash
    git clone <repository_url>
    cd nodes_speed_test
    ```
3.  **Configure**: Create and edit your `config.toml` file as described above.
4.  **Run**:
    ```bash
    cargo run
    ```

## Report

After the test completes, a report will be printed to the console, similar to this:

```
--- Final Report ---
Total Races: 10, Draws/Errors: 0
------------------------------------------------------------------------------------------------------------------
Accelerator          | Wins     | Win %    | Success    | Failure    | Avg Latency (ms)   | Min Latency (ms)   | Max Latency (ms)
------------------------------------------------------------------------------------------------------------------
Accelerator 1        | 7        | 70.00%   | 10         | 0          | 150.20             | 120                | 180
Accelerator 2        | 3        | 30.00%   | 10         | 0          | 180.50             | 160                | 200
------------------------------------------------------------------------------------------------------------------
```

*   **Wins**: The number of times the accelerator's transaction was the first to be confirmed.
*   **Win %**: The percentage of total races won by the accelerator.
*   **Success**: The number of times the API call to the accelerator was successful.
*   **Failure**: The number of times the API call to the accelerator failed.
*   **Avg Latency (ms)**: The average time it took to get a successful API response from the accelerator.
*   **Min Latency (ms)**: The minimum API response time.
*   **Max Latency (ms)**: The maximum API response time.
