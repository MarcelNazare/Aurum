use dotenvy::dotenv;
use std::env;

fn env_usize(key: &str, default: usize) -> usize {
    env::var(key).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}
fn env_str(key: &str, default: &str) -> String {
    env::var(key).unwrap_or_else(|_| default.to_string())
}

#[tokio::main]
async fn main() {
    if dotenv().is_err() {
        eprintln!("· no .env file — using process environment only");
    }

    let api_key = match env::var("TWELVE_DATA_API_KEY") {
        Ok(k) if !k.is_empty() && k != "your_api_key_here" => k,
        _ => {
            eprintln!("✖ TWELVE_DATA_API_KEY missing or placeholder.");
            eprintln!("  1. cp .env.example .env");
            eprintln!("  2. paste your key: https://twelvedata.com/register");
            std::process::exit(1);
        }
    };

    let default_symbol = env_str("AURUM_SYMBOL", "XAU/USD");

    let mut port: u16 = env_usize("AURUM_PORT", 3000) as u16;
    let args: Vec<String> = env::args().collect();
    let mut i = 1;
    while i < args.len() {
        if args[i] == "--port" && i + 1 < args.len() {
            port = match args[i + 1].parse() {
                Ok(p) => p,
                Err(_) => { eprintln!("✖ invalid port"); std::process::exit(1); }
            };
            i += 2;
        } else if args[i] == "help" || args[i] == "-h" || args[i] == "--help" {
            println!("aurum-web — Aurum web UI\n");
            println!("  aurum-web [--port N]");
            return;
        } else {
            i += 1;
        }
    }

    if let Err(e) = aurum::server::run(api_key, default_symbol, port).await {
        eprintln!("✖ {}", e);
        std::process::exit(1);
    }
}