#![forbid(unsafe_code)]

use chrono::{NaiveDate, NaiveDateTime};
use serde::Serialize;

pub mod server;

// ═══════════════════════════════════════════════════════════════════════════
// DATA MODEL
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Candle {
    pub ts: i64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Side { Bull, Bear }

impl Side {
    pub fn opp(self) -> Side { if self == Side::Bull { Side::Bear } else { Side::Bull } }
    pub fn label(self) -> &'static str { if self == Side::Bull { "LONG" } else { "SHORT" } }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwingSide { High, Low }

#[derive(Debug, Clone, Copy)]
pub struct Swing { pub idx: usize, pub price: f64, pub side: SwingSide }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum StructureKind { Bos, Choch, Mss }

impl StructureKind {
    pub fn label(self) -> &'static str {
        match self {
            StructureKind::Bos => "BOS",
            StructureKind::Choch => "ChoCh",
            StructureKind::Mss => "MSS",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct StructureEvent {
    pub idx: usize,
    pub kind: StructureKind,
    pub level: f64,
    pub direction: Side,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Sweep {
    pub idx: usize,
    pub level: f64,
    pub direction: Side,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Fvg {
    pub start_idx: usize,
    pub top: f64,
    pub bottom: f64,
    pub side: Side,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FvgStatus { Fresh, Partial, Mitigated }

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IfvgEntry {
    pub idx: usize,
    pub fvg: Fvg,
    pub side: Side,
    pub entry_price: f64,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Zone {
    pub start_idx: usize,
    pub top: f64,
    pub bottom: f64,
    pub side: Side,
    pub label: &'static str,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Signal {
    pub strategy: &'static str,
    pub side: Side,
    pub idx: usize,
    pub entry: f64,
    pub stop: f64,
    pub tp1: f64,
    pub tp2: f64,
    pub tp3: f64,
    pub score: u32,
    pub reasons: Vec<String>,
    pub entry_top: f64,
    pub entry_bot: f64,
}

// ═══════════════════════════════════════════════════════════════════════════
// DETECTION PRIMITIVES
// ═══════════════════════════════════════════════════════════════════════════

pub fn aggregate(c: &[Candle], n: usize) -> Vec<Candle> {
    let n = n.max(1);
    c.chunks(n).map(|ch| Candle {
        ts: ch[0].ts,
        open: ch[0].open,
        high: ch.iter().map(|x| x.high).fold(f64::MIN, f64::max),
        low:  ch.iter().map(|x| x.low ).fold(f64::MAX, f64::min),
        close: ch.last().unwrap().close,
    }).collect()
}

pub fn swings(c: &[Candle], lookback: usize) -> Vec<Swing> {
    let mut out = Vec::new();
    if c.len() < 2 * lookback + 1 { return out; }
    for i in lookback..c.len() - lookback {
        let mut hi = true; let mut lo = true;
        for j in (i - lookback)..=i + lookback {
            if j == i { continue; }
            if c[j].high >= c[i].high { hi = false; }
            if c[j].low  <= c[i].low  { lo = false; }
        }
        if hi { out.push(Swing { idx: i, price: c[i].high, side: SwingSide::High }); }
        if lo { out.push(Swing { idx: i, price: c[i].low,  side: SwingSide::Low  }); }
    }
    out.sort_by_key(|s| s.idx);
    out
}

pub fn liquidity_sweeps(c: &[Candle], lookback: usize) -> Vec<Sweep> {
    let sw = swings(c, lookback);
    let mut out = Vec::new();
    for s in sw {
        for i in s.idx + 1..c.len() {
            match s.side {
                SwingSide::High => {
                    if c[i].high > s.price {
                        if c[i].close < s.price {
                            out.push(Sweep { idx: i, level: s.price, direction: Side::Bull });
                        }
                        break;
                    }
                }
                SwingSide::Low => {
                    if c[i].low < s.price {
                        if c[i].close > s.price {
                            out.push(Sweep { idx: i, level: s.price, direction: Side::Bear });
                        }
                        break;
                    }
                }
            }
        }
    }
    out.sort_by_key(|s| s.idx);
    out
}

pub fn detect_structure(c: &[Candle], lookback: usize) -> Vec<StructureEvent> {
    let sw = swings(c, lookback);
    let sweeps = liquidity_sweeps(c, lookback);
    let mut events = Vec::new();
    let mut last_high: Option<f64> = None;
    let mut last_low:  Option<f64> = None;
    let mut trend: Option<Side> = None;
    let mut si = 0;

    for i in 0..c.len() {
        while si < sw.len() && sw[si].idx + lookback <= i {
            match sw[si].side {
                SwingSide::High => last_high = Some(sw[si].price),
                SwingSide::Low  => last_low  = Some(sw[si].price),
            }
            si += 1;
        }
        if let Some(h) = last_high {
            if c[i].close > h {
                let base = if trend == Some(Side::Bear) { StructureKind::Choch } else { StructureKind::Bos };
                let sweep_recent = sweeps.iter().any(|s| s.idx + 5 >= i && s.idx < i && s.direction == Side::Bull);
                let kind = if base == StructureKind::Choch && sweep_recent { StructureKind::Mss } else { base };
                events.push(StructureEvent { idx: i, kind, level: h, direction: Side::Bull });
                trend = Some(Side::Bull); last_high = None;
            }
        }
        if let Some(l) = last_low {
            if c[i].close < l {
                let base = if trend == Some(Side::Bull) { StructureKind::Choch } else { StructureKind::Bos };
                let sweep_recent = sweeps.iter().any(|s| s.idx + 5 >= i && s.idx < i && s.direction == Side::Bear);
                let kind = if base == StructureKind::Choch && sweep_recent { StructureKind::Mss } else { base };
                events.push(StructureEvent { idx: i, kind, level: l, direction: Side::Bear });
                trend = Some(Side::Bear); last_low = None;
            }
        }
    }
    events
}

pub fn find_fvgs(c: &[Candle]) -> Vec<Fvg> {
    let mut out = Vec::new();
    if c.len() < 3 { return out; }
    for i in 2..c.len() {
        let (c0, c2) = (&c[i - 2], &c[i]);
        if c0.high < c2.low {
            out.push(Fvg { start_idx: i - 2, top: c2.low, bottom: c0.high, side: Side::Bull });
        } else if c0.low > c2.high {
            out.push(Fvg { start_idx: i - 2, top: c0.low, bottom: c2.high, side: Side::Bear });
        }
    }
    out
}

pub fn fvg_status_at(c: &[Candle], f: &Fvg, at: usize) -> FvgStatus {
    let at = at.min(c.len());
    let mut touched = false;
    for j in (f.start_idx + 2)..at {
        match f.side {
            Side::Bull => {
                if c[j].close < f.bottom { return FvgStatus::Mitigated; }
                if c[j].low <= f.top { touched = true; }
            }
            Side::Bear => {
                if c[j].close > f.top { return FvgStatus::Mitigated; }
                if c[j].high >= f.bottom { touched = true; }
            }
        }
    }
    if touched { FvgStatus::Partial } else { FvgStatus::Fresh }
}

pub fn ifvg_entries(c: &[Candle]) -> Vec<IfvgEntry> {
    let fvgs = find_fvgs(c);
    let mut out = Vec::new();
    for f in &fvgs {
        let mut violated: Option<usize> = None;
        for j in f.start_idx + 3..c.len() {
            let broke = match f.side {
                Side::Bull => c[j].close < f.bottom,
                Side::Bear => c[j].close > f.top,
            };
            if broke { violated = Some(j); break; }
        }
        let Some(v) = violated else { continue };
        for j in (v + 1)..c.len() {
            match f.side {
                Side::Bull => {
                    if c[j].high >= f.bottom && c[j].close < f.bottom {
                        out.push(IfvgEntry { idx: j, fvg: *f, side: Side::Bear, entry_price: c[j].close });
                        break;
                    }
                    if c[j].close > f.top { break; }
                }
                Side::Bear => {
                    if c[j].low <= f.top && c[j].close > f.top {
                        out.push(IfvgEntry { idx: j, fvg: *f, side: Side::Bull, entry_price: c[j].close });
                        break;
                    }
                    if c[j].close < f.bottom { break; }
                }
            }
        }
    }
    out.sort_by_key(|e| e.idx);
    out
}

pub fn order_block_before(c: &[Candle], idx: usize, dir: Side, max_lb: usize) -> Option<Zone> {
    let start = idx.saturating_sub(max_lb);
    for i in (start..idx).rev() {
        let bear = c[i].close < c[i].open;
        let bull = c[i].close > c[i].open;
        match dir {
            Side::Bull if bear => return Some(Zone { start_idx: i, top: c[i].high, bottom: c[i].low, side: Side::Bull, label: "DEMAND" }),
            Side::Bear if bull => return Some(Zone { start_idx: i, top: c[i].high, bottom: c[i].low, side: Side::Bear, label: "SUPPLY" }),
            _ => {}
        }
    }
    None
}

pub fn atr(c: &[Candle], period: usize) -> Vec<f64> {
    let mut out = vec![0.0; c.len()];
    for i in 1..c.len() {
        let tr = (c[i].high - c[i].low)
            .max((c[i].high - c[i - 1].close).abs())
            .max((c[i].low  - c[i - 1].close).abs());
        let n = i.min(period) as f64;
        out[i] = (out[i - 1] * (n - 1.0) + tr) / n;
    }
    out
}

pub fn htf_bias(htf: &[Candle]) -> (Side, Option<StructureEvent>) {
    let ev = detect_structure(htf, 2);
    match ev.last() {
        Some(e) => (e.direction, Some(*e)),
        None => (Side::Bull, None),
    }
}

pub fn premium_discount(c: &[Candle], idx: usize, lookback: usize) -> f64 {
    let start = idx.saturating_sub(lookback);
    let lo = c[start..=idx].iter().map(|x| x.low ).fold(f64::MAX, f64::min);
    let hi = c[start..=idx].iter().map(|x| x.high).fold(f64::MIN, f64::max);
    ((c[idx].close - lo) / (hi - lo).max(1e-9)).clamp(0.0, 1.0)
}

// ═══════════════════════════════════════════════════════════════════════════
// CONFLUENCE SCORING (internal)
// ═══════════════════════════════════════════════════════════════════════════

struct Factors {
    sweep: bool, mss: bool, ifvg: bool, fvg: bool, zone: bool,
    fresh: bool, htf_align: bool, pdc: bool, vol: bool,
}

fn score(f: &Factors) -> (u32, Vec<String>) {
    let mut s = 0u32;
    let mut r = Vec::new();
    if f.sweep     { s += 2; r.push("sweep".into()); }
    if f.mss       { s += 2; r.push("MSS".into()); }
    if f.ifvg      { s += 2; r.push("IFVG".into()); }
    if f.fvg       { s += 1; r.push("FVG".into()); }
    if f.zone      { s += 1; r.push("S/D".into()); }
    if f.fresh     { s += 1; r.push("fresh".into()); }
    if f.htf_align { s += 1; r.push("HTF-align".into()); }
    if f.pdc       { s += 1; r.push("P/D".into()); }
    if f.vol       { s += 1; r.push("vol".into()); }
    (s, r)
}

const MAX_WAIT_HTF: usize = 30;
const HTF_CONFIRM_WINDOW: usize = 5;

#[allow(clippy::too_many_arguments)]
fn build_signal(
    strategy: &'static str, side: Side, idx: usize,
    entry: f64, stop_raw: f64, atr_val: f64,
    _tp3_struct: Option<f64>, factors: &Factors,
    entry_top: f64, entry_bot: f64,
) -> Option<Signal> {
    let buffer = atr_val * 0.15;
    let stop = match side { Side::Bull => stop_raw - buffer, Side::Bear => stop_raw + buffer };
    let risk = (entry - stop).abs();
    if risk < 1e-9 { return None; }
    let (tp1, tp2) = match side {
        Side::Bull => (entry + risk, entry + 2.0 * risk),
        Side::Bear => (entry - risk, entry - 2.0 * risk),
    };
    let tp3 = match side { Side::Bull => entry + 3.0 * risk, Side::Bear => entry - 3.0 * risk };
    let (score_val, reasons) = score(factors);
    Some(Signal {
        strategy, side, idx, entry, stop, tp1, tp2, tp3,
        score: score_val, reasons, entry_top, entry_bot,
    })
}

// ═══════════════════════════════════════════════════════════════════════════
// STRATEGIES
// ═══════════════════════════════════════════════════════════════════════════

pub fn strategy1(ltf: &[Candle], htf: &[Candle], htf_n: usize) -> Vec<Signal> {
    let mut out = Vec::new();
    if htf.len() < 20 { return out; }
    let sweeps  = liquidity_sweeps(htf, 2);
    let structs = detect_structure(htf, 2);
    let (bias, _) = htf_bias(htf);
    let ltf_ifvg = ifvg_entries(ltf);
    let ltf_fvgs = find_fvgs(ltf);
    let ltf_atr  = atr(ltf, 14);

    for sw in &sweeps {
        let want = sw.direction.opp();
        let Some(mss) = structs.iter().find(|e|
            e.idx > sw.idx && e.idx <= sw.idx + MAX_WAIT_HTF
            && e.direction == want && e.kind != StructureKind::Bos
        ) else { continue };
        let Some(ob) = order_block_before(htf, mss.idx, want, 10) else { continue };

        let mut ret: Option<usize> = None;
        for i in (mss.idx + 1)..=((mss.idx + MAX_WAIT_HTF).min(htf.len() - 1)) {
            let touched = match want { Side::Bull => htf[i].low <= ob.top, Side::Bear => htf[i].high >= ob.bottom };
            if touched { ret = Some(i); break; }
        }
        let Some(r) = ret else { continue };

        let ltf_start = r * htf_n;
        let ltf_end   = ((r + HTF_CONFIRM_WINDOW) * htf_n).min(ltf.len());
        if ltf_end <= ltf_start + 2 { continue; }

        let ltf_structs = detect_structure(&ltf[..ltf_end], 1);
        let Some(lm) = ltf_structs.iter().find(|e|
            e.idx >= ltf_start && e.direction == want && e.kind != StructureKind::Bos
        ) else { continue };

        let Some(ifv) = ltf_ifvg.iter().find(|e| e.idx > lm.idx && e.idx < ltf_end && e.side == want) else { continue };

        let pdc = premium_discount(htf, r, 40);
        let pdc_ok = match want { Side::Bull => pdc < 0.55, Side::Bear => pdc > 0.45 };
        let atr_ok = ltf_atr.get(ifv.idx).map(|a| *a > 0.05).unwrap_or(false);

        let factors = Factors {
            sweep: true, mss: mss.kind == StructureKind::Mss, ifvg: true,
            fvg: ltf_fvgs.iter().any(|f| f.side == want && f.start_idx < ifv.idx),
            zone: true, fresh: true,
            htf_align: want == bias, pdc: pdc_ok, vol: atr_ok,
        };

        let stop_swing = match want { Side::Bull => ob.bottom, Side::Bear => ob.top };
        let atr_v = ltf_atr.get(ifv.idx).copied().unwrap_or(0.0);
        if let Some(sig) = build_signal(
            "S1 Sweep+MSS+IFVG", want, ifv.idx, ifv.entry_price, stop_swing, atr_v,
            None, &factors, ob.top, ob.bottom,
        ) { out.push(sig); }
    }
    out
}

pub fn strategy2(ltf: &[Candle], htf: &[Candle], htf_n: usize) -> Vec<Signal> {
    let mut out = Vec::new();
    if htf.len() < 20 { return out; }
    let structs = detect_structure(htf, 2);
    let (bias, _) = htf_bias(htf);
    let ltf_ifvg = ifvg_entries(ltf);
    let ltf_atr  = atr(ltf, 14);

    let mut pairs = Vec::new();
    for w in structs.windows(2) {
        let (a, b) = (w[0], w[1]);
        if a.kind == StructureKind::Bos && b.kind == StructureKind::Bos
            && a.direction == b.direction && b.idx > a.idx {
            pairs.push((a, b));
        }
    }

    for (a, b) in pairs {
        let dir = a.direction;
        let Some(zone) = order_block_before(htf, a.idx, dir, 15) else { continue };

        let mut ret: Option<usize> = None;
        for i in (b.idx + 1)..=((b.idx + MAX_WAIT_HTF).min(htf.len() - 1)) {
            let touched = match dir { Side::Bull => htf[i].low <= zone.top, Side::Bear => htf[i].high >= zone.bottom };
            if touched { ret = Some(i); break; }
        }
        let Some(r) = ret else { continue };

        let ltf_start = r * htf_n;
        let ltf_end   = ((r + HTF_CONFIRM_WINDOW) * htf_n).min(ltf.len());
        if ltf_end <= ltf_start + 2 { continue; }

        let ltf_structs = detect_structure(&ltf[..ltf_end], 1);
        let Some(lm) = ltf_structs.iter().find(|e|
            e.idx >= ltf_start && e.direction == dir && e.kind != StructureKind::Bos
        ) else { continue };

        let Some(breaker) = order_block_before(&ltf[..ltf_end], lm.idx, dir, 5) else { continue };

        let pdc = premium_discount(htf, r, 40);
        let pdc_ok = match dir { Side::Bull => pdc < 0.55, Side::Bear => pdc > 0.45 };
        let atr_ok = ltf_atr.get(lm.idx).map(|x| *x > 0.05).unwrap_or(false);

        let factors = Factors {
            sweep: false, mss: lm.kind == StructureKind::Mss,
            ifvg: ltf_ifvg.iter().any(|e| e.side == dir && e.idx > lm.idx && e.idx < ltf_end),
            fvg: find_fvgs(&ltf[..ltf_end]).iter().any(|f| f.side == dir),
            zone: true, fresh: true,
            htf_align: dir == bias, pdc: pdc_ok, vol: atr_ok,
        };

        let entry = match dir { Side::Bull => breaker.top, Side::Bear => breaker.bottom };
        let stop_swing = match dir { Side::Bull => breaker.bottom, Side::Bear => breaker.top };
        let atr_v = ltf_atr.get(lm.idx).copied().unwrap_or(0.0);
        if let Some(sig) = build_signal(
            "S2 SD+Breaker", dir, lm.idx, entry, stop_swing, atr_v,
            None, &factors, breaker.top, breaker.bottom,
        ) { out.push(sig); }
    }
    out
}

pub fn strategy3(ltf: &[Candle], htf: &[Candle], htf_n: usize) -> Vec<Signal> {
    let mut out = Vec::new();
    if htf.len() < 20 { return out; }
    let structs = detect_structure(htf, 2);
    let fvgs    = find_fvgs(htf);
    let ltf_ifvg = ifvg_entries(ltf);
    let (bias, _) = htf_bias(htf);
    let ltf_atr  = atr(ltf, 14);

    for (i, ev) in structs.iter().enumerate() {
        if ev.kind == StructureKind::Bos { continue; }
        let prior_bos = structs[..i].iter()
            .filter(|e| e.direction == ev.direction.opp() && e.kind == StructureKind::Bos)
            .count();
        if prior_bos < 2 { continue; }

        let Some(fvg) = fvgs.iter().find(|f| f.side == ev.direction
            && f.start_idx + 5 >= ev.idx && f.start_idx <= ev.idx + 3)
        else { continue };
        if fvg_status_at(htf, fvg, htf.len() - 1) == FvgStatus::Mitigated { continue; }

        let mut ret: Option<usize> = None;
        for j in (ev.idx + 1)..=((ev.idx + MAX_WAIT_HTF).min(htf.len() - 1)) {
            if fvg_status_at(htf, fvg, j) == FvgStatus::Mitigated { break; }
            let touched = match ev.direction { Side::Bull => htf[j].low <= fvg.top, Side::Bear => htf[j].high >= fvg.bottom };
            if touched { ret = Some(j); break; }
        }
        let Some(r) = ret else { continue };

        let ltf_start = r * htf_n;
        let ltf_end   = ((r + HTF_CONFIRM_WINDOW) * htf_n).min(ltf.len());
        if ltf_end <= ltf_start + 2 { continue; }

        let Some(ifv) = ltf_ifvg.iter().find(|e|
            e.idx >= ltf_start && e.idx < ltf_end && e.side == ev.direction
        ) else { continue };

        let pdc = premium_discount(htf, r, 40);
        let pdc_ok = match ev.direction { Side::Bull => pdc < 0.55, Side::Bear => pdc > 0.45 };
        let atr_ok = ltf_atr.get(ifv.idx).map(|x| *x > 0.05).unwrap_or(false);

        let factors = Factors {
            sweep: false, mss: ev.kind == StructureKind::Mss, ifvg: true, fvg: true,
            zone: false, fresh: true,
            htf_align: ev.direction == bias, pdc: pdc_ok, vol: atr_ok,
        };

        let stop_swing = match ev.direction { Side::Bull => fvg.bottom, Side::Bear => fvg.top };
        let atr_v = ltf_atr.get(ifv.idx).copied().unwrap_or(0.0);
        if let Some(sig) = build_signal(
            "S3 ChoCh+FVG+IFVG", ev.direction, ifv.idx, ifv.entry_price, stop_swing, atr_v,
            None, &factors, fvg.top, fvg.bottom,
        ) { out.push(sig); }
    }
    out
}

// ═══════════════════════════════════════════════════════════════════════════
// DATA FETCHER
// ═══════════════════════════════════════════════════════════════════════════

pub async fn fetch_twelvedata(
    api_key: &str, symbol: &str, interval: &str, outputsize: usize,
) -> Result<Vec<Candle>, Box<dyn std::error::Error + Send + Sync>> {
    let encoded = symbol.replace('/', "%2F");
    let url = format!(
        "https://api.twelvedata.com/time_series?symbol={}&interval={}&outputsize={}&apikey={}",
        encoded, interval, outputsize, api_key
    );
    let resp: serde_json::Value = reqwest::get(&url).await?.json().await?;
    if resp.get("status").and_then(|s| s.as_str()) == Some("error") {
        let msg = resp.get("message").and_then(|m| m.as_str()).unwrap_or("unknown");
        return Err(format!("Twelve Data API error: {}", msg).into());
    }
    let values = resp.get("values").and_then(|v| v.as_array())
        .ok_or("response missing 'values' array")?;

    let mut candles: Vec<Candle> = values.iter().filter_map(|row| {
        let dt_str = row.get("datetime")?.as_str()?;
        let ts = if let Ok(dt) = NaiveDateTime::parse_from_str(dt_str, "%Y-%m-%d %H:%M:%S") {
            dt.and_utc().timestamp()
        } else if let Ok(d) = NaiveDate::parse_from_str(dt_str, "%Y-%m-%d") {
            d.and_hms_opt(0, 0, 0).map(|x| x.and_utc().timestamp()).unwrap_or(0)
        } else { 0 };
        let f = |k: &str| row.get(k)?.as_str()?.parse::<f64>().ok();
        Some(Candle { ts, open: f("open")?, high: f("high")?, low: f("low")?, close: f("close")? })
    }).collect();

    candles.reverse();
    Ok(candles)
}