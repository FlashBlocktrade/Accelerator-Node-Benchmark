use std::collections::HashMap;

// This struct is now defined in race.rs, but we use its fields here.
pub use crate::race::SendAttempt;

#[derive(Debug, Default, Clone, Copy)]
struct Stats {
    wins: u32,
    success_count: u32,
    failure_count: u32,
    total_latency: u128,
    min_latency: u128,
    max_latency: u128,
}

pub fn generate_report(
    send_attempts: &[SendAttempt],
    wins: &HashMap<String, u32>,
    draws: u32,
    num_transactions: u32,
    accelerator_names: &[String],
) {
    let mut stats: HashMap<String, Stats> = HashMap::new();

    // Initialize stats for all accelerators to ensure they appear in the report
    for name in accelerator_names {
        stats.entry(name.clone()).or_default();
    }

    // Populate wins
    for (name, win_count) in wins {
        if let Some(s) = stats.get_mut(name) {
            s.wins = *win_count;
        }
    }

    // Calculate latency stats from all send attempts
    for attempt in send_attempts {
        if let Some(s) = stats.get_mut(&attempt.name) {
            if attempt.is_success {
                s.success_count += 1;
                let latency_ms = attempt.latency.as_millis();
                s.total_latency += latency_ms;
                if s.min_latency == 0 || latency_ms < s.min_latency {
                    s.min_latency = latency_ms;
                }
                if latency_ms > s.max_latency {
                    s.max_latency = latency_ms;
                }
            } else {
                s.failure_count += 1;
            }
        }
    }

    println!("\n--- Final Report ---");
    println!("Total Races: {}, Draws/Errors: {}", num_transactions, draws);
    println!("------------------------------------------------------------------------------------------------------------------");
    println!("{:<20} | {:<8} | {:<8} | {:<10} | {:<10} | {:<18} | {:<18} | {:<18}", 
             "Accelerator", "Wins", "Win %", "Success", "Failure", "Avg Latency (ms)", "Min Latency (ms)", "Max Latency (ms)");
    println!("------------------------------------------------------------------------------------------------------------------");

    let mut sorted_stats: Vec<_> = stats.into_iter().collect();
    // Sort by wins, descending
    sorted_stats.sort_by(|a, b| b.1.wins.cmp(&a.1.wins));

    for (name, s) in sorted_stats {
        let win_percentage = (s.wins as f64 / num_transactions as f64) * 100.0;
        let avg_latency = if s.success_count > 0 { s.total_latency as f64 / s.success_count as f64 } else { 0.0 };
        
        let min_latency_display = if s.success_count > 0 { s.min_latency.to_string() } else { "N/A".to_string() };
        let max_latency_display = if s.success_count > 0 { s.max_latency.to_string() } else { "N/A".to_string() };


        println!("{:<20} | {:<8} | {:<9.2}% | {:<10} | {:<10} | {:<18.2} | {:<18} | {:<18}", 
                 name, s.wins, win_percentage, s.success_count, s.failure_count, avg_latency, min_latency_display, max_latency_display);
    }
    println!("------------------------------------------------------------------------------------------------------------------");
}
