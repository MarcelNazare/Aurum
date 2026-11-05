use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{Html, IntoResponse, Json, Response},
    routing::get,
    Router,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::net::SocketAddr;
use std::time::Duration;

use crate::{
    aggregate, detect_structure, find_fvgs, fvg_status_at, htf_bias, ifvg_entries,
    liquidity_sweeps, order_block_before, strategy1, strategy2, strategy3,
    Candle, Fvg, FvgStatus, IfvgEntry, Side, Signal, StructureEvent, StructureKind, Sweep, Zone,
};

const INDEX_HTML: &str = include_str!("../pages/index.html");

#[derive(Clone)]
struct AppState {
    api_key: String,
    default_symbol: String,
}

#[derive(Deserialize)]
struct AnalyzeQuery {
    symbol: Option<String>,
    interval: Option<String>,
    outputsize: Option<usize>,
    htf_ratio: Option<usize>,
    min_score: Option<u32>,
    api_key: Option<String>,
}

#[derive(Serialize)]
struct BiasInfo {
    direction: Side,
    kind: Option<StructureKind>,
    idx: Option<usize>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AnalyzeResponse {
    symbol: String,
    interval: String,
    candles: Vec<Candle>,
    bias: BiasInfo,
    structures: Vec<StructureEvent>,
    sweeps: Vec<Sweep>,
    fvgs: Vec<Fvg>,
    ifvgs: Vec<IfvgEntry>,
    zones: Vec<Zone>,
    signals: Vec<Signal>,
}

enum AppError {
    Fetch(String),
    BadRequest(String),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, msg) = match self {
            AppError::Fetch(m)      => (StatusCode::BAD_GATEWAY, m),
            AppError::BadRequest(m) => (StatusCode::BAD_REQUEST, m),
        };
        (status, Json(serde_json::json!({ "error": msg }))).into_response()
    }
}

pub async fn run(
    api_key: String,
    default_symbol: String,
    port: u16,
    open_browser: bool,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let state = AppState { api_key, default_symbol };

    let app = Router::new()
        .route("/", get(index))
        .route("/api/analyze", get(analyze))
        .with_state(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| format!("could not bind {}: {} — is another instance running?", addr, e))?;

    let url = format!("http://{}/", addr);
    println!("✔ Aurum web UI: {}", url);
    println!("  key loaded from .env — never exposed to the browser");

    if open_browser {
        let url_bg = url.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(250)).await;
            match webbrowser::open(&url_bg) {
                Ok(_)  => println!("✔ opened {} in your default browser", url_bg),
                Err(e) => eprintln!("· could not open browser automatically: {}", e),
            }
        });
    }

    axum::serve(listener, app).await?;
    Ok(())
}

async fn index() -> Html<&'static str> {
    Html(INDEX_HTML)
}

async fn analyze(
    State(state): State<AppState>,
    Query(q): Query<AnalyzeQuery>,
) -> Result<Json<AnalyzeResponse>, AppError> {
    let key = q.api_key.filter(|k| !k.is_empty()).unwrap_or_else(|| state.api_key.clone());
    let symbol     = q.symbol.unwrap_or_else(|| state.default_symbol.clone());
    let interval   = q.interval.unwrap_or_else(|| "15min".into());
    let outputsize = q.outputsize.unwrap_or(500).clamp(100, 5000);
    let htf_ratio  = q.htf_ratio.unwrap_or(5).clamp(2, 50);
    let min_score  = q.min_score.unwrap_or(6).min(12);

    let candles = crate::fetch_twelvedata(&key, &symbol, &interval, outputsize)
        .await
        .map_err(|e| AppError::Fetch(e.to_string()))?;

    if candles.len() < 100 {
        return Err(AppError::BadRequest(format!(
            "only {} candles returned; increase outputsize or use a larger interval",
            candles.len()
        )));
    }

    let htf = aggregate(&candles, htf_ratio);
    let (bias_dir, bias_event) = htf_bias(&htf);

    let structures = detect_structure(&candles, 2);
    let sweeps     = liquidity_sweeps(&candles, 2);
    let all_ifvgs  = ifvg_entries(&candles);

    let fresh_fvgs: Vec<Fvg> = {
        let mut v: Vec<Fvg> = find_fvgs(&candles)
            .into_iter()
            .filter(|f| fvg_status_at(&candles, f, candles.len()) == FvgStatus::Fresh)
            .collect();
        if v.len() > 8 {
            v = v.split_off(v.len() - 8);
        }
        v
    };

    let recent_ifvgs: Vec<IfvgEntry> = {
        let mut v = all_ifvgs;
        if v.len() > 6 {
            v = v.split_off(v.len() - 6);
        }
        v
    };

    let mut signals = Vec::new();
    signals.extend(strategy1(&candles, &htf, htf_ratio));
    signals.extend(strategy2(&candles, &htf, htf_ratio));
    signals.extend(strategy3(&candles, &htf, htf_ratio));
    
    signals.retain(|s| s.score >= min_score);
    crate::deduplicate_signals(&mut signals);
    crate::evaluate_signals(&candles, &mut signals);
    crate::enrich_signals(&mut signals, &candles, &interval);
    signals.sort_by_key(|s| s.idx);

    let mut zones = Vec::new();
    let mut seen: HashSet<(usize, bool)> = HashSet::new();
    for ev in structures.iter().rev().take(6) {
        if let Some(z) = order_block_before(&candles, ev.idx, ev.direction, 10) {
            if seen.insert((z.start_idx, z.side == Side::Bull)) {
                zones.push(z);
            }
        }
    }

    let bias = match bias_event {
        Some(e) => BiasInfo { direction: bias_dir, kind: Some(e.kind), idx: Some(e.idx) },
        None    => BiasInfo { direction: bias_dir, kind: None, idx: None },
    };

    Ok(Json(AnalyzeResponse {
        symbol, interval, candles, bias,
        structures, sweeps,
        fvgs: fresh_fvgs,
        ifvgs: recent_ifvgs,
        zones, signals,
    }))
}