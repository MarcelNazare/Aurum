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
    let mut open_browser = true;

    let args: Vec<String> = env::args().collect();
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--port" if i + 1 < args.len() => {
                port = match args[i + 1].parse() {
                    Ok(p) => p,
                    Err(_) => {
                        eprintln!("✖ invalid port");
                        std::process::exit(1);
                    }
                };
                i += 2;
            }
            "--no-open" => {
                open_browser = false;
                i += 1;
            }
            "help" | "-h" | "--help" => {
                println!("aurum-web — Aurum web UI\n");
                println!("  aurum-web [--port N] [--no-open]\n");
                println!("  --port N    port to listen on (default 3000 or AURUM_PORT)");
                println!("  --no-open   do not auto-launch the browser");
                return;
            }
            other => {
                eprintln!("· ignoring unknown argument: {}", other);
                i += 1;
            }
        }
    }

    if let Err(e) = aurum::server::run(api_key, default_symbol, port, open_browser).await {
        eprintln!("✖ {}", e);
        std::process::exit(1);
    }
}