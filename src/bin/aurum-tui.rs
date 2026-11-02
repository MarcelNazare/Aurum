use dotenvy::dotenv;
use std::env;

use aurum::{
    aggregate, detect_structure, fetch_twelvedata, find_fvgs, fvg_status_at, htf_bias,
    ifvg_entries, liquidity_sweeps, order_block_before, strategy1, strategy2, strategy3,
    Candle, Fvg, FvgStatus, Side, StructureKind, Zone,
};

// ═══════════════════════════════════════════════════════════════════════════
// ENV HELPERS
// ═══════════════════════════════════════════════════════════════════════════

fn env_usize(key: &str, default: usize) -> usize {
    env::var(key).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}
fn env_str(key: &str, default: &str) -> String {
    env::var(key).unwrap_or_else(|_| default.to_string())
}

// ═══════════════════════════════════════════════════════════════════════════
// TERMINAL CHART
// ═══════════════════════════════════════════════════════════════════════════

const RESET: &str = "\x1b[0m";

fn fg(c: &str) -> &'static str {
    match c {
        "green" => "\x1b[32m", "red" => "\x1b[31m", "yellow" => "\x1b[33m",
        "cyan" => "\x1b[36m", "magenta" => "\x1b[35m", "white" => "\x1b[37m",
        "bright_green" => "\x1b[92m", "bright_red" => "\x1b[91m",
        "bright_yellow" => "\x1b[93m", "bright_cyan" => "\x1b[96m",
        "bright_magenta" => "\x1b[95m", "bright_white" => "\x1b[97m",
        "dim" => "\x1b[2m", _ => "",
    }
}
fn bg(c: &str) -> &'static str {
    match c {
        "dark_green" => "\x1b[48;5;22m", "dark_red" => "\x1b[48;5;52m",
        "dark_blue" => "\x1b[48;5;17m", "dark_purple" => "\x1b[48;5;53m",
        "dark_orange" => "\x1b[48;5;94m", _ => "",
    }
}

#[derive(Clone, Copy)]
struct Cell { ch: char, f: &'static str, b: &'static str }
impl Cell {
    fn blank() -> Self { Cell { ch: ' ', f: "", b: "" } }
    fn render(&self) -> String {
        let mut s = String::new();
        if !self.f.is_empty() { s.push_str(fg(self.f)); }
        if !self.b.is_empty() { s.push_str(bg(self.b)); }
        s.push(self.ch);
        if !self.f.is_empty() || !self.b.is_empty() { s.push_str(RESET); }
        s
    }
}

fn to_row(price: f64, hi: f64, lo: f64, h: usize) -> usize {
    if (hi - lo).abs() < 1e-9 { return h / 2; }
    let frac = (hi - price) / (hi - lo);
    ((frac * (h - 1) as f64).round() as usize).min(h - 1)
}

struct Chart<'a> {
    candles: &'a [Candle], start: usize, end: usize,
    width: usize, height: usize, hi: f64, lo: f64,
    grid: Vec<Vec<Cell>>,
}

impl<'a> Chart<'a> {
    fn new(candles: &'a [Candle], start: usize, end: usize, width: usize, height: usize) -> Self {
        let slice = &candles[start..end];
        let mut hi = f64::MIN; let mut lo = f64::MAX;
        for c in slice { hi = hi.max(c.high); lo = lo.min(c.low); }
        let pad = (hi - lo) * 0.05 + 1e-9;
        Chart {
            candles, start, end, width, height,
            hi: hi + pad, lo: lo - pad,
            grid: vec![vec![Cell::blank(); width]; height],
        }
    }
    fn set(&mut self, x: usize, y: usize, ch: char, f: &'static str, b: &'static str) {
        if x < self.width && y < self.height { self.grid[y][x] = Cell { ch, f, b }; }
    }
    fn set_bg(&mut self, y: usize, x0: usize, x1: usize, b: &'static str) {
        if y >= self.height { return; }
        for x in x0..x1.min(self.width) { self.grid[y][x].b = b; }
    }
    fn to_col(&self, idx: usize) -> usize {
        if self.end <= self.start { return 0; }
        if idx < self.start { return 0; }
        if idx >= self.end { return self.width - 1; }
        ((idx - self.start) * self.width / (self.end - self.start)).min(self.width - 1)
    }
    fn draw_zone(&mut self, z: &Zone, color: &'static str) {
        let yt = to_row(z.top, self.hi, self.lo, self.height);
        let yb = to_row(z.bottom, self.hi, self.lo, self.height);
        let x0 = self.to_col(z.start_idx);
        for y in yt..=yb { self.set_bg(y, x0, self.width, color); }
    }
    fn draw_fvg(&mut self, f: &Fvg, color: &'static str) {
        let yt = to_row(f.top, self.hi, self.lo, self.height);
        let yb = to_row(f.bottom, self.hi, self.lo, self.height);
        let x0 = self.to_col(f.start_idx);
        for y in yt..=yb {
            self.set_bg(y, x0, self.width, color);
            for x in x0..self.width {
                if self.grid[y][x].ch == ' ' { self.grid[y][x].ch = '░'; }
            }
        }
    }
    fn draw_candles(&mut self) {
        for x in 0..self.width {
            let c0 = self.start + x * (self.end - self.start) / self.width;
            let c1 = self.start + ((x + 1) * (self.end - self.start)) / self.width;
            if c0 >= self.end { break; }
            let c1 = c1.max(c0 + 1).min(self.end);
            let slice = &self.candles[c0..c1];
            let o = slice[0].open;
            let cl = slice.last().unwrap().close;
            let hi = slice.iter().map(|x| x.high).fold(f64::MIN, f64::max);
            let lo = slice.iter().map(|x| x.low).fold(f64::MAX, f64::min);
            let color = if cl >= o { "green" } else { "red" };
            let yt = to_row(hi, self.hi, self.lo, self.height);
            let yb = to_row(lo, self.hi, self.lo, self.height);
            let bt = to_row(o.max(cl), self.hi, self.lo, self.height);
            let bb = to_row(o.min(cl), self.hi, self.lo, self.height);
            for y in yt..=yb {
                let ch = if y >= bt && y <= bb { '█' } else { '│' };
                let b = self.grid[y][x].b;
                self.set(x, y, ch, color, b);
            }
        }
    }
    fn draw_marker(&mut self, idx: usize, price: f64, ch: char, color: &'static str) {
        if idx < self.start || idx >= self.end { return; }
        let x = self.to_col(idx);
        let y = to_row(price, self.hi, self.lo, self.height);
        let b = self.grid[y][x].b;
        self.set(x, y, ch, color, b);
    }
    fn draw_hband(&mut self, price: f64, from_idx: usize, ch: char, f: &'static str, b: &'static str) {
        let x0 = self.to_col(from_idx);
        let y = to_row(price, self.hi, self.lo, self.height);
        for x in x0..self.width { self.set(x, y, ch, f, b); }
    }
    fn render(&self) -> String {
        let mut out = String::with_capacity((10 + self.width) * self.height * 4);
        for y in 0..self.height {
            let p = self.hi - (y as f64 / (self.height - 1) as f64) * (self.hi - self.lo);
            out.push_str(&format!("{:>8.2} │", p));
            for x in 0..self.width { out.push_str(&self.grid[y][x].render()); }
            out.push('\n');
        }
        out
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// MAIN
// ═══════════════════════════════════════════════════════════════════════════

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
            return;
        }
    };

    let args: Vec<String> = env::args().collect();
    if matches!(args.get(1).map(|s| s.as_str()), Some("help") | Some("-h") | Some("--help")) {
        println!("aurum-tui — Aurum terminal scanner\n");
        println!("  aurum-tui [interval]");
        println!("  e.g. aurum-tui 1h");
        return;
    }

    let symbol     = env_str("AURUM_SYMBOL", "XAU/USD");
    let interval   = args.get(1).cloned().unwrap_or_else(|| env_str("AURUM_INTERVAL", "15min"));
    let outputsize = env_usize("AURUM_OUTPUTSIZE", 500);
    let window     = env_usize("AURUM_WINDOW", 100);
    let htf_n      = env_usize("AURUM_HTF_RATIO", 5);
    let min_score  = env_usize("AURUM_MIN_SCORE", 6) as u32;

    println!("◈ Aurum · {} · {} · {} bars · window={} · HTF=1:{} · min_score={}",
             symbol, interval, outputsize, window, htf_n, min_score);

    let candles = match fetch_twelvedata(&api_key, &symbol, &interval, outputsize).await {
        Ok(c) => { println!("✔ fetched {} real {} candles\n", c.len(), symbol); c }
        Err(e) => { eprintln!("✖ fetch failed: {}", e); return; }
    };
    if candles.len() < 100 {
        eprintln!("✖ need ≥100 candles (got {}); raise AURUM_OUTPUTSIZE", candles.len());
        return;
    }

    let htf = aggregate(&candles, htf_n);
    let (bias, bias_event) = htf_bias(&htf);

    let ltf_structs = detect_structure(&candles, 2);
    let ltf_sweeps  = liquidity_sweeps(&candles, 2);
    let ltf_fvgs    = find_fvgs(&candles);
    let ltf_ifvgs   = ifvg_entries(&candles);

    let mut signals = Vec::new();
    signals.extend(strategy1(&candles, &htf, htf_n));
    signals.extend(strategy2(&candles, &htf, htf_n));
    signals.extend(strategy3(&candles, &htf, htf_n));
    signals.retain(|s| s.score >= min_score);
    signals.sort_by_key(|s| s.idx);

    // ── Header ──
    println!("╔══════════════════════════════════════════════════════════════════════╗");
    println!("║  AURUM · {} · {} · {} bars                                  ", symbol, interval, candles.len());
    println!("╚══════════════════════════════════════════════════════════════════════╝");

    let bias_line = match bias_event {
        Some(e) => format!("{} (last HTF {} @ bar {})", bias.label(), e.kind.label(), e.idx),
        None    => bias.label().to_string(),
    };
    println!(
        "  BIAS: {}{}{}\n",
        fg(if bias == Side::Bull { "bright_green" } else { "bright_red" }),
        bias_line, RESET
    );

    // ── Chart ──
    let end = candles.len();
    let start = end.saturating_sub(window.max(20));
    let cw = (end - start).min(100);
    let chh = 32;
    let mut chart = Chart::new(&candles, start, end, cw, chh);

    for ev in ltf_structs.iter().rev().take(4) {
        if let Some(z) = order_block_before(&candles, ev.idx, ev.direction, 10) {
            let color = if z.side == Side::Bull { "dark_green" } else { "dark_red" };
            chart.draw_zone(&z, color);
        }
    }
    let fresh_fvgs: Vec<_> = ltf_fvgs.iter()
        .filter(|f| fvg_status_at(&candles, f, end) != FvgStatus::Mitigated)
        .rev().take(6).collect();
    for f in &fresh_fvgs {
        let color = if f.side == Side::Bull { "dark_blue" } else { "dark_purple" };
        chart.draw_fvg(f, color);
    }
    for iv in ltf_ifvgs.iter().rev().take(4) {
        chart.draw_fvg(&iv.fvg, "dark_orange");
    }
    chart.draw_candles();

    for ev in &ltf_structs {
        let (ch, color) = match ev.kind {
            StructureKind::Bos => (if ev.direction == Side::Bull { '▲' } else { '▼' }, "bright_yellow"),
            StructureKind::Choch => ('◆', "bright_yellow"),
            StructureKind::Mss => ('✦', "bright_magenta"),
        };
        chart.draw_marker(ev.idx, ev.level, ch, color);
    }
    for sw in &ltf_sweeps { chart.draw_marker(sw.idx, sw.level, '○', "bright_cyan"); }
    for iv in &ltf_ifvgs { chart.draw_marker(iv.idx, iv.entry_price, '◈', "bright_magenta"); }

    for sig in &signals {
        chart.draw_hband(sig.entry, sig.idx, '═', "bright_green", "");
        chart.draw_hband(sig.stop,  sig.idx, '─', "bright_red",   "");
        chart.draw_hband(sig.tp1,   sig.idx, '─', "bright_yellow","");
        chart.draw_hband(sig.tp2,   sig.idx, '─', "bright_yellow","");
        chart.draw_hband(sig.tp3,   sig.idx, '─', "yellow",       "");
    }
    let last = candles[end - 1].close;
    chart.draw_hband(last, end - 1, '▶', "bright_white", "");

    println!("{}", chart.render());

    // ── Legend ──
    print!("  LEGEND  ");
    print!("{}▲▼{} BOS  ", fg("bright_yellow"), RESET);
    print!("{}◆{} ChoCh  ", fg("bright_yellow"), RESET);
    print!("{}✦{} MSS  ", fg("bright_magenta"), RESET);
    print!("{}○{} Sweep  ", fg("bright_cyan"), RESET);
    print!("{}◈{} IFVG  ", fg("bright_magenta"), RESET);
    print!("{}░{} FVG  ", fg("dim"), RESET);
    println!();
    println!(
        "  {}██{} Supply  {}██{} Demand  {}▶{} Price  {}══{} Entry  {}──{} SL  {}──{} TP",
        fg("red"), RESET, fg("green"), RESET,
        fg("bright_white"), RESET,
        fg("bright_green"), RESET, fg("bright_red"), RESET, fg("bright_yellow"), RESET,
    );
    println!();

    // ── Signals ──
    if signals.is_empty() {
        println!("  No signals above threshold ({}) on this dataset.\n", min_score);
        return;
    }
    println!("  ┌─ SIGNALS ─────────────────────────────────────────────────────────────────────────┐");
    println!("  │ {:<3} {:<18} {:<5} {:>9} {:>9} {:>9} {:>9} {:>9} {:>5} │",
             "#", "STRATEGY", "SIDE", "ENTRY", "STOP", "TP1", "TP2", "TP3", "SCORE");
    println!("  ├───────────────────────────────────────────────────────────────────────────────────┤");
    for (k, s) in signals.iter().enumerate() {
        let color = if s.side == Side::Bull { "bright_green" } else { "bright_red" };
        println!("  │ {:<3} {:<18} {}{:<5}{} {:>9.2} {:>9.2} {:>9.2} {:>9.2} {:>9.2} {:>5} │",
                 k + 1, s.strategy, fg(color), s.side.label(), RESET,
                 s.entry, s.stop, s.tp1, s.tp2, s.tp3, s.score);
        println!("  │     {}reasons: {}{}", fg("dim"), s.reasons.join(" · "), RESET);
    }
    println!("  └───────────────────────────────────────────────────────────────────────────────────┘");
    println!("\n  Total: {} signals\n", signals.len());
}