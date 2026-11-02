#![forbid(unsafe_code)]

use chrono::{NaiveDate, NaiveDateTime};
use dotenvy::dotenv;
use std::env;

// ═══════════════════════════════════════════════════════════════════════════
// DATA MODEL
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Copy)]
pub struct Candle {
    pub ts: i64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Bull,
    Bear,
}

impl Side {
    fn opp(self) -> Side {
        if self == Side::Bull { Side::Bear } else { Side::Bull }
    }
    fn label(self) -> &'static str {
        if self == Side::Bull { "LONG" } else { "SHORT" }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwingSide {
    High,
    Low,
}

#[derive(Debug, Clone, Copy)]
pub struct Swing {
    pub idx: usize,
    pub price: f64,
    pub side: SwingSide,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StructureKind {
    Bos,
    Choch,
    Mss,
}

impl StructureKind {
    fn label(self) -> &'static str {
        match self {
            StructureKind::Bos => "BOS",
            StructureKind::Choch => "ChoCh",
            StructureKind::Mss => "MSS",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct StructureEvent {
    pub idx: usize,
    pub kind: StructureKind,
    pub level: f64,
    pub direction: Side,
}

#[derive(Debug, Clone, Copy)]
pub struct Sweep {
    pub idx: usize,
    pub level: f64,
    pub direction: Side,
}

#[derive(Debug, Clone, Copy)]
pub struct Fvg {
    pub start_idx: usize,
    pub top: f64,
    pub bottom: f64,
    pub side: Side,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FvgStatus {
    Fresh,
    Partial,
    Mitigated,
}

#[derive(Debug, Clone, Copy)]
pub struct IfvgEntry {
    pub idx: usize,
    pub fvg: Fvg,
    pub side: Side,
    pub entry_price: f64,
}

#[derive(Debug, Clone, Copy)]
pub struct Zone {
    pub start_idx: usize,
    pub top: f64,
    pub bottom: f64,
    pub side: Side,
    pub label: &'static str,
}

#[derive(Debug, Clone)]
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
    c.chunks(n)
        .map(|ch| Candle {
            ts: ch[0].ts,
            open: ch[0].open,
            high: ch.iter().map(|x| x.high).fold(f64::MIN, f64::max),
            low: ch.iter().map(|x| x.low).fold(f64::MAX, f64::min),
            close: ch.last().unwrap().close,
        })
        .collect()
}

pub fn swings(c: &[Candle], lookback: usize) -> Vec<Swing> {
    let mut out = Vec::new();
    if c.len() < 2 * lookback + 1 {
        return out;
    }
    for i in lookback..c.len() - lookback {
        let mut hi = true;
        let mut lo = true;
        for j in (i - lookback)..=i + lookback {
            if j == i {
                continue;
            }
            if c[j].high >= c[i].high {
                hi = false;
            }
            if c[j].low <= c[i].low {
                lo = false;
            }
        }
        if hi {
            out.push(Swing {
                idx: i,
                price: c[i].high,
                side: SwingSide::High,
            });
        }
        if lo {
            out.push(Swing {
                idx: i,
                price: c[i].low,
                side: SwingSide::Low,
            });
        }
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
                            out.push(Sweep {
                                idx: i,
                                level: s.price,
                                direction: Side::Bull,
                            });
                        }
                        break;
                    }
                }
                SwingSide::Low => {
                    if c[i].low < s.price {
                        if c[i].close > s.price {
                            out.push(Sweep {
                                idx: i,
                                level: s.price,
                                direction: Side::Bear,
                            });
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
    let mut last_low: Option<f64> = None;
    let mut trend: Option<Side> = None;
    let mut si = 0;

    for i in 0..c.len() {
        while si < sw.len() && sw[si].idx + lookback <= i {
            match sw[si].side {
                SwingSide::High => last_high = Some(sw[si].price),
                SwingSide::Low => last_low = Some(sw[si].price),
            }
            si += 1;
        }
        if let Some(h) = last_high {
            if c[i].close > h {
                let base = if trend == Some(Side::Bear) {
                    StructureKind::Choch
                } else {
                    StructureKind::Bos
                };
                let sweep_recent = sweeps
                    .iter()
                    .any(|s| s.idx + 5 >= i && s.idx < i && s.direction == Side::Bull);
                let kind = if base == StructureKind::Choch && sweep_recent {
                    StructureKind::Mss
                } else {
                    base
                };
                events.push(StructureEvent {
                    idx: i,
                    kind,
                    level: h,
                    direction: Side::Bull,
                });
                trend = Some(Side::Bull);
                last_high = None;
            }
        }
        if let Some(l) = last_low {
            if c[i].close < l {
                let base = if trend == Some(Side::Bull) {
                    StructureKind::Choch
                } else {
                    StructureKind::Bos
                };
                let sweep_recent = sweeps
                    .iter()
                    .any(|s| s.idx + 5 >= i && s.idx < i && s.direction == Side::Bear);
                let kind = if base == StructureKind::Choch && sweep_recent {
                    StructureKind::Mss
                } else {
                    base
                };
                events.push(StructureEvent {
                    idx: i,
                    kind,
                    level: l,
                    direction: Side::Bear,
                });
                trend = Some(Side::Bear);
                last_low = None;
            }
        }
    }
    events
}

pub fn find_fvgs(c: &[Candle]) -> Vec<Fvg> {
    let mut out = Vec::new();
    if c.len() < 3 {
        return out;
    }
    for i in 2..c.len() {
        let (c0, c2) = (&c[i - 2], &c[i]);
        if c0.high < c2.low {
            out.push(Fvg {
                start_idx: i - 2,
                top: c2.low,
                bottom: c0.high,
                side: Side::Bull,
            });
        } else if c0.low > c2.high {
            out.push(Fvg {
                start_idx: i - 2,
                top: c0.low,
                bottom: c2.high,
                side: Side::Bear,
            });
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
                if c[j].close < f.bottom {
                    return FvgStatus::Mitigated;
                }
                if c[j].low <= f.top {
                    touched = true;
                }
            }
            Side::Bear => {
                if c[j].close > f.top {
                    return FvgStatus::Mitigated;
                }
                if c[j].high >= f.bottom {
                    touched = true;
                }
            }
        }
    }
    if touched {
        FvgStatus::Partial
    } else {
        FvgStatus::Fresh
    }
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
            if broke {
                violated = Some(j);
                break;
            }
        }
        let Some(v) = violated else { continue };
        for j in (v + 1)..c.len() {
            match f.side {
                Side::Bull => {
                    if c[j].high >= f.bottom && c[j].close < f.bottom {
                        out.push(IfvgEntry {
                            idx: j,
                            fvg: *f,
                            side: Side::Bear,
                            entry_price: c[j].close,
                        });
                        break;
                    }
                    if c[j].close > f.top {
                        break;
                    }
                }
                Side::Bear => {
                    if c[j].low <= f.top && c[j].close > f.top {
                        out.push(IfvgEntry {
                            idx: j,
                            fvg: *f,
                            side: Side::Bull,
                            entry_price: c[j].close,
                        });
                        break;
                    }
                    if c[j].close < f.bottom {
                        break;
                    }
                }
            }
        }
    }
    out.sort_by_key(|e| e.idx);
    out
}

pub fn order_block_before(
    c: &[Candle],
    idx: usize,
    dir: Side,
    max_lb: usize,
) -> Option<Zone> {
    let start = idx.saturating_sub(max_lb);
    for i in (start..idx).rev() {
        let bear = c[i].close < c[i].open;
        let bull = c[i].close > c[i].open;
        match dir {
            Side::Bull if bear => {
                return Some(Zone {
                    start_idx: i,
                    top: c[i].high,
                    bottom: c[i].low,
                    side: Side::Bull,
                    label: "DEMAND",
                })
            }
            Side::Bear if bull => {
                return Some(Zone {
                    start_idx: i,
                    top: c[i].high,
                    bottom: c[i].low,
                    side: Side::Bear,
                    label: "SUPPLY",
                })
            }
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
            .max((c[i].low - c[i - 1].close).abs());
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
    let lo = c[start..=idx]
        .iter()
        .map(|x| x.low)
        .fold(f64::MAX, f64::min);
    let hi = c[start..=idx]
        .iter()
        .map(|x| x.high)
        .fold(f64::MIN, f64::max);
    ((c[idx].close - lo) / (hi - lo).max(1e-9)).clamp(0.0, 1.0)
}

// ═══════════════════════════════════════════════════════════════════════════
// CONFLUENCE SCORING
// ═══════════════════════════════════════════════════════════════════════════

struct Factors {
    sweep: bool,
    mss: bool,
    ifvg: bool,
    fvg: bool,
    zone: bool,
    fresh: bool,
    htf_align: bool,
    pdc: bool,
    vol: bool,
}

fn score(f: &Factors) -> (u32, Vec<String>) {
    let mut s = 0u32;
    let mut r = Vec::new();
    if f.sweep {
        s += 2;
        r.push("sweep".into());
    }
    if f.mss {
        s += 2;
        r.push("MSS".into());
    }
    if f.ifvg {
        s += 2;
        r.push("IFVG".into());
    }
    if f.fvg {
        s += 1;
        r.push("FVG".into());
    }
    if f.zone {
        s += 1;
        r.push("S/D".into());
    }
    if f.fresh {
        s += 1;
        r.push("fresh".into());
    }
    if f.htf_align {
        s += 1;
        r.push("HTF-align".into());
    }
    if f.pdc {
        s += 1;
        r.push("P/D".into());
    }
    if f.vol {
        s += 1;
        r.push("vol".into());
    }
    (s, r)
}

const MAX_WAIT_HTF: usize = 30;
const HTF_CONFIRM_WINDOW: usize = 5;

#[allow(clippy::too_many_arguments)]
fn build_signal(
    strategy: &'static str,
    side: Side,
    idx: usize,
    entry: f64,
    stop_raw: f64,
    atr_val: f64,
    tp3_struct: Option<f64>,
    factors: &Factors,
    entry_top: f64,
    entry_bot: f64,
) -> Option<Signal> {
    let buffer = atr_val * 0.15;
    let stop = match side {
        Side::Bull => stop_raw - buffer,
        Side::Bear => stop_raw + buffer,
    };
    let risk = (entry - stop).abs();
    if risk < 1e-9 {
        return None;
    }
    let (tp1, tp2) = match side {
        Side::Bull => (entry + risk, entry + 2.0 * risk),
        Side::Bear => (entry - risk, entry - 2.0 * risk),
    };
    let tp3 = tp3_struct.unwrap_or(match side {
        Side::Bull => entry + 3.0 * risk,
        Side::Bear => entry - 3.0 * risk,
    });
    let (score_val, reasons) = score(factors);
    Some(Signal {
        strategy,
        side,
        idx,
        entry,
        stop,
        tp1,
        tp2,
        tp3,
        score: score_val,
        reasons,
        entry_top,
        entry_bot,
    })
}

// ═══════════════════════════════════════════════════════════════════════════
// STRATEGY 1 — Sweep + MSS + OB → LTF MSS + IFVG
// ═══════════════════════════════════════════════════════════════════════════

fn strategy1(ltf: &[Candle], htf: &[Candle], htf_n: usize) -> Vec<Signal> {
    let mut out = Vec::new();
    if htf.len() < 20 {
        return out;
    }

    let sweeps = liquidity_sweeps(htf, 2);
    let structs = detect_structure(htf, 2);
    let (bias, _) = htf_bias(htf);
    let ltf_ifvg = ifvg_entries(ltf);
    let ltf_fvgs = find_fvgs(ltf);
    let ltf_atr = atr(ltf, 14);

    for sw in &sweeps {
        let want = sw.direction.opp();
        let Some(mss) = structs.iter().find(|e| {
            e.idx > sw.idx
                && e.idx <= sw.idx + MAX_WAIT_HTF
                && e.direction == want
                && e.kind != StructureKind::Bos
        }) else {
            continue;
        };
        let Some(ob) = order_block_before(htf, mss.idx, want, 10) else {
            continue;
        };

        let mut ret: Option<usize> = None;
        for i in (mss.idx + 1)..=((mss.idx + MAX_WAIT_HTF).min(htf.len() - 1)) {
            let touched = match want {
                Side::Bull => htf[i].low <= ob.top,
                Side::Bear => htf[i].high >= ob.bottom,
            };
            if touched {
                ret = Some(i);
                break;
            }
        }
        let Some(r) = ret else { continue };

        let ltf_start = r * htf_n;
        let ltf_end = ((r + HTF_CONFIRM_WINDOW) * htf_n).min(ltf.len());
        if ltf_end <= ltf_start + 2 {
            continue;
        }

        let ltf_structs = detect_structure(&ltf[..ltf_end], 1);
        let Some(lm) = ltf_structs.iter().find(|e| {
            e.idx >= ltf_start && e.direction == want && e.kind != StructureKind::Bos
        }) else {
            continue;
        };

        let Some(ifv) = ltf_ifvg
            .iter()
            .find(|e| e.idx > lm.idx && e.idx < ltf_end && e.side == want)
        else {
            continue;
        };

        let pdc = premium_discount(htf, r, 40);
        let pdc_ok = match want {
            Side::Bull => pdc < 0.55,
            Side::Bear => pdc > 0.45,
        };
        let atr_ok = ltf_atr.get(ifv.idx).map(|a| *a > 0.05).unwrap_or(false);

        let factors = Factors {
            sweep: true,
            mss: mss.kind == StructureKind::Mss,
            ifvg: true,
            fvg: ltf_fvgs
                .iter()
                .any(|f| f.side == want && f.start_idx < ifv.idx),
            zone: true,
            fresh: true,
            htf_align: want == bias,
            pdc: pdc_ok,
            vol: atr_ok,
        };

        let stop_swing = match want {
            Side::Bull => ob.bottom,
            Side::Bear => ob.top,
        };
        let entry = ifv.entry_price;
        let atr_v = ltf_atr.get(ifv.idx).copied().unwrap_or(0.0);
        if let Some(sig) = build_signal(
            "S1 Sweep+MSS+IFVG",
            want,
            ifv.idx,
            entry,
            stop_swing,
            atr_v,
            None,
            &factors,
            ob.top,
            ob.bottom,
        ) {
            out.push(sig);
        }
    }
    out
}

// ═══════════════════════════════════════════════════════════════════════════
// STRATEGY 2 — Double BOS → S/D zone → 5m MSS + breaker
// ═══════════════════════════════════════════════════════════════════════════

fn strategy2(ltf: &[Candle], htf: &[Candle], htf_n: usize) -> Vec<Signal> {
    let mut out = Vec::new();
    if htf.len() < 20 {
        return out;
    }

    let structs = detect_structure(htf, 2);
    let (bias, _) = htf_bias(htf);
    let ltf_ifvg = ifvg_entries(ltf);
    let ltf_atr = atr(ltf, 14);

    let mut pairs = Vec::new();
    for w in structs.windows(2) {
        let (a, b) = (w[0], w[1]);
        if a.kind == StructureKind::Bos
            && b.kind == StructureKind::Bos
            && a.direction == b.direction
            && b.idx > a.idx
        {
            pairs.push((a, b));
        }
    }

    for (a, b) in pairs {
        let dir = a.direction;
        let Some(zone) = order_block_before(htf, a.idx, dir, 15) else {
            continue;
        };

        let mut ret: Option<usize> = None;
        for i in (b.idx + 1)..=((b.idx + MAX_WAIT_HTF).min(htf.len() - 1)) {
            let touched = match dir {
                Side::Bull => htf[i].low <= zone.top,
                Side::Bear => htf[i].high >= zone.bottom,
            };
            if touched {
                ret = Some(i);
                break;
            }
        }
        let Some(r) = ret else { continue };

        let ltf_start = r * htf_n;
        let ltf_end = ((r + HTF_CONFIRM_WINDOW) * htf_n).min(ltf.len());
        if ltf_end <= ltf_start + 2 {
            continue;
        }

        let ltf_structs = detect_structure(&ltf[..ltf_end], 1);
        let Some(lm) = ltf_structs.iter().find(|e| {
            e.idx >= ltf_start && e.direction == dir && e.kind != StructureKind::Bos
        }) else {
            continue;
        };

        let Some(breaker) = order_block_before(&ltf[..ltf_end], lm.idx, dir, 5) else {
            continue;
        };

        let pdc = premium_discount(htf, r, 40);
        let pdc_ok = match dir {
            Side::Bull => pdc < 0.55,
            Side::Bear => pdc > 0.45,
        };
        let atr_ok = ltf_atr.get(lm.idx).map(|x| *x > 0.05).unwrap_or(false);

        let factors = Factors {
            sweep: false,
            mss: lm.kind == StructureKind::Mss,
            ifvg: ltf_ifvg
                .iter()
                .any(|e| e.side == dir && e.idx > lm.idx && e.idx < ltf_end),
            fvg: find_fvgs(&ltf[..ltf_end]).iter().any(|f| f.side == dir),
            zone: true,
            fresh: true,
            htf_align: dir == bias,
            pdc: pdc_ok,
            vol: atr_ok,
        };

        let entry = match dir {
            Side::Bull => breaker.top,
            Side::Bear => breaker.bottom,
        };
        let stop_swing = match dir {
            Side::Bull => breaker.bottom,
            Side::Bear => breaker.top,
        };
        let atr_v = ltf_atr.get(lm.idx).copied().unwrap_or(0.0);
        if let Some(sig) = build_signal(
            "S2 SD+Breaker",
            dir,
            lm.idx,
            entry,
            stop_swing,
            atr_v,
            None,
            &factors,
            breaker.top,
            breaker.bottom,
        ) {
            out.push(sig);
        }
    }
    out
}

// ═══════════════════════════════════════════════════════════════════════════
// STRATEGY 3 — HTF ChoCh + unmitigated FVG → LTF IFVG
// ═══════════════════════════════════════════════════════════════════════════

fn strategy3(ltf: &[Candle], htf: &[Candle], htf_n: usize) -> Vec<Signal> {
    let mut out = Vec::new();
    if htf.len() < 20 {
        return out;
    }

    let structs = detect_structure(htf, 2);
    let fvgs = find_fvgs(htf);
    let ltf_ifvg = ifvg_entries(ltf);
    let (bias, _) = htf_bias(htf);
    let ltf_atr = atr(ltf, 14);

    for (i, ev) in structs.iter().enumerate() {
        if ev.kind == StructureKind::Bos {
            continue;
        }
        let prior_bos = structs[..i]
            .iter()
            .filter(|e| e.direction == ev.direction.opp() && e.kind == StructureKind::Bos)
            .count();
        if prior_bos < 2 {
            continue;
        }

        let Some(fvg) = fvgs.iter().find(|f| {
            f.side == ev.direction && f.start_idx + 5 >= ev.idx && f.start_idx <= ev.idx + 3
        }) else {
            continue;
        };
        if fvg_status_at(htf, fvg, htf.len() - 1) == FvgStatus::Mitigated {
            continue;
        }

        let mut ret: Option<usize> = None;
        for j in (ev.idx + 1)..=((ev.idx + MAX_WAIT_HTF).min(htf.len() - 1)) {
            if fvg_status_at(htf, fvg, j) == FvgStatus::Mitigated {
                break;
            }
            let touched = match ev.direction {
                Side::Bull => htf[j].low <= fvg.top,
                Side::Bear => htf[j].high >= fvg.bottom,
            };
            if touched {
                ret = Some(j);
                break;
            }
        }
        let Some(r) = ret else { continue };

        let ltf_start = r * htf_n;
        let ltf_end = ((r + HTF_CONFIRM_WINDOW) * htf_n).min(ltf.len());
        if ltf_end <= ltf_start + 2 {
            continue;
        }

        let Some(ifv) = ltf_ifvg
            .iter()
            .find(|e| e.idx >= ltf_start && e.idx < ltf_end && e.side == ev.direction)
        else {
            continue;
        };

        let pdc = premium_discount(htf, r, 40);
        let pdc_ok = match ev.direction {
            Side::Bull => pdc < 0.55,
            Side::Bear => pdc > 0.45,
        };
        let atr_ok = ltf_atr.get(ifv.idx).map(|x| *x > 0.05).unwrap_or(false);

        let factors = Factors {
            sweep: false,
            mss: ev.kind == StructureKind::Mss,
            ifvg: true,
            fvg: true,
            zone: false,
            fresh: true,
            htf_align: ev.direction == bias,
            pdc: pdc_ok,
            vol: atr_ok,
        };

        let entry = ifv.entry_price;
        let stop_swing = match ev.direction {
            Side::Bull => fvg.bottom,
            Side::Bear => fvg.top,
        };
        let atr_v = ltf_atr.get(ifv.idx).copied().unwrap_or(0.0);
        if let Some(sig) = build_signal(
            "S3 ChoCh+FVG+IFVG",
            ev.direction,
            ifv.idx,
            entry,
            stop_swing,
            atr_v,
            None,
            &factors,
            fvg.top,
            fvg.bottom,
        ) {
            out.push(sig);
        }
    }
    out
}

// ═══════════════════════════════════════════════════════════════════════════
// TERMINAL CHART
// ═══════════════════════════════════════════════════════════════════════════

const RESET: &str = "\x1b[0m";

fn fg(c: &str) -> &'static str {
    match c {
        "green" => "\x1b[32m",
        "red" => "\x1b[31m",
        "yellow" => "\x1b[33m",
        "cyan" => "\x1b[36m",
        "magenta" => "\x1b[35m",
        "white" => "\x1b[37m",
        "bright_green" => "\x1b[92m",
        "bright_red" => "\x1b[91m",
        "bright_yellow" => "\x1b[93m",
        "bright_cyan" => "\x1b[96m",
        "bright_magenta" => "\x1b[95m",
        "bright_white" => "\x1b[97m",
        "dim" => "\x1b[2m",
        _ => "",
    }
}

fn bg(c: &str) -> &'static str {
    match c {
        "dark_green" => "\x1b[48;5;22m",
        "dark_red" => "\x1b[48;5;52m",
        "dark_blue" => "\x1b[48;5;17m",
        "dark_purple" => "\x1b[48;5;53m",
        "dark_orange" => "\x1b[48;5;94m",
        _ => "",
    }
}

#[derive(Clone, Copy)]
struct Cell {
    ch: char,
    f: &'static str,
    b: &'static str,
}

impl Cell {
    fn blank() -> Self {
        Cell {
            ch: ' ',
            f: "",
            b: "",
        }
    }
    fn render(&self) -> String {
        let mut s = String::new();
        if !self.f.is_empty() {
            s.push_str(fg(self.f));
        }
        if !self.b.is_empty() {
            s.push_str(bg(self.b));
        }
        s.push(self.ch);
        if !self.f.is_empty() || !self.b.is_empty() {
            s.push_str(RESET);
        }
        s
    }
}

fn to_row(price: f64, hi: f64, lo: f64, h: usize) -> usize {
    if (hi - lo).abs() < 1e-9 {
        return h / 2;
    }
    let frac = (hi - price) / (hi - lo);
    ((frac * (h - 1) as f64).round() as usize).min(h - 1)
}

struct Chart<'a> {
    candles: &'a [Candle],
    start: usize,
    end: usize,
    width: usize,
    height: usize,
    hi: f64,
    lo: f64,
    grid: Vec<Vec<Cell>>,
}

impl<'a> Chart<'a> {
    fn new(
        candles: &'a [Candle],
        start: usize,
        end: usize,
        width: usize,
        height: usize,
    ) -> Self {
        let slice = &candles[start..end];
        let mut hi = f64::MIN;
        let mut lo = f64::MAX;
        for c in slice {
            hi = hi.max(c.high);
            lo = lo.min(c.low);
        }
        let pad = (hi - lo) * 0.05 + 1e-9;
        let hi = hi + pad;
        let lo = lo - pad;
        Chart {
            candles,
            start,
            end,
            width,
            height,
            hi,
            lo,
            grid: vec![vec![Cell::blank(); width]; height],
        }
    }

    fn set(&mut self, x: usize, y: usize, ch: char, f: &'static str, b: &'static str) {
        if x < self.width && y < self.height {
            self.grid[y][x] = Cell { ch, f, b };
        }
    }

    fn set_bg(&mut self, y: usize, x0: usize, x1: usize, b: &'static str) {
        if y >= self.height {
            return;
        }
        for x in x0..x1.min(self.width) {
            self.grid[y][x].b = b;
        }
    }

    fn to_col(&self, idx: usize) -> usize {
        if self.end <= self.start {
            return 0;
        }
        if idx < self.start {
            return 0;
        }
        if idx >= self.end {
            return self.width - 1;
        }
        ((idx - self.start) * self.width / (self.end - self.start)).min(self.width - 1)
    }

    fn draw_zone(&mut self, z: &Zone, color: &'static str) {
        let yt = to_row(z.top, self.hi, self.lo, self.height);
        let yb = to_row(z.bottom, self.hi, self.lo, self.height);
        let x0 = self.to_col(z.start_idx);
        for y in yt..=yb {
            self.set_bg(y, x0, self.width, color);
        }
    }

    fn draw_fvg(&mut self, f: &Fvg, color: &'static str) {
        let yt = to_row(f.top, self.hi, self.lo, self.height);
        let yb = to_row(f.bottom, self.hi, self.lo, self.height);
        let x0 = self.to_col(f.start_idx);
        for y in yt..=yb {
            self.set_bg(y, x0, self.width, color);
            for x in x0..self.width {
                if self.grid[y][x].ch == ' ' {
                    self.grid[y][x].ch = '░';
                }
            }
        }
    }

    fn draw_candles(&mut self) {
        for x in 0..self.width {
            let c0 = self.start + x * (self.end - self.start) / self.width;
            let c1 = self.start + ((x + 1) * (self.end - self.start)) / self.width;
            if c0 >= self.end {
                break;
            }
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
                self.set(x, y, ch, color, self.grid[y][x].b);
            }
        }
    }

    fn draw_marker(&mut self, idx: usize, price: f64, ch: char, color: &'static str) {
        if idx < self.start || idx >= self.end {
            return;
        }
        let x = self.to_col(idx);
        let y = to_row(price, self.hi, self.lo, self.height);
        let b = self.grid[y][x].b;
        self.set(x, y, ch, color, b);
    }

    fn draw_hband(
        &mut self,
        price: f64,
        from_idx: usize,
        ch: char,
        f: &'static str,
        b: &'static str,
    ) {
        let x0 = self.to_col(from_idx);
        let y = to_row(price, self.hi, self.lo, self.height);
        for x in x0..self.width {
            self.set(x, y, ch, f, b);
        }
    }

    fn render(&self) -> String {
        let mut out = String::with_capacity((10 + self.width) * self.height * 4);
        for y in 0..self.height {
            let p = self.hi - (y as f64 / (self.height - 1) as f64) * (self.hi - self.lo);
            out.push_str(&format!("{:>8.2} │", p));
            for x in 0..self.width {
                out.push_str(&self.grid[y][x].render());
            }
            out.push('\n');
        }
        out
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// DATA FETCHER — Twelve Data
// ═══════════════════════════════════════════════════════════════════════════

async fn fetch_twelvedata(
    api_key: &str,
    symbol: &str,
    interval: &str,
    outputsize: usize,
) -> Result<Vec<Candle>, Box<dyn std::error::Error>> {
    let encoded = symbol.replace('/', "%2F");
    let url = format!(
        "https://api.twelvedata.com/time_series?symbol={}&interval={}&outputsize={}&apikey={}",
        encoded, interval, outputsize, api_key
    );

    let resp: serde_json::Value = reqwest::get(&url).await?.json().await?;

    if resp.get("status").and_then(|s| s.as_str()) == Some("error") {
        let msg = resp
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("unknown error");
        return Err(format!("Twelve Data API error: {}", msg).into());
    }

    let values = resp
        .get("values")
        .and_then(|v| v.as_array())
        .ok_or("response missing 'values' array")?;

    let mut candles: Vec<Candle> = values
        .iter()
        .filter_map(|row| {
            let dt_str = row.get("datetime")?.as_str()?;
            let ts = if let Ok(dt) = NaiveDateTime::parse_from_str(dt_str, "%Y-%m-%d %H:%M:%S") {
                dt.and_utc().timestamp()
            } else if let Ok(d) = NaiveDate::parse_from_str(dt_str, "%Y-%m-%d") {
                d.and_hms_opt(0, 0, 0)
                    .map(|x| x.and_utc().timestamp())
                    .unwrap_or(0)
            } else {
                0
            };
            let f = |k: &str| row.get(k)?.as_str()?.parse::<f64>().ok();
            Some(Candle {
                ts,
                open: f("open")?,
                high: f("high")?,
                low: f("low")?,
                close: f("close")?,
            })
        })
        .collect();

    candles.reverse(); // Twelve Data returns newest-first
    Ok(candles)
}

// ═══════════════════════════════════════════════════════════════════════════
// ENV HELPERS
// ═══════════════════════════════════════════════════════════════════════════

fn env_usize(key: &str, default: usize) -> usize {
    env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn env_str(key: &str, default: &str) -> String {
    env::var(key).unwrap_or_else(|_| default.to_string())
}

// ═══════════════════════════════════════════════════════════════════════════
// MAIN
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::main]
async fn main() {
    if dotenv().is_err() {
        eprintln!("· no .env file found — reading process environment only");
    }

    let api_key = match env::var("TWELVE_DATA_API_KEY") {
        Ok(k) if !k.is_empty() && k != "your_api_key_here" => k,
        _ => {
            eprintln!("✖ TWELVE_DATA_API_KEY missing or placeholder.");
            eprintln!("  1. cp .env.example .env");
            eprintln!("  2. paste your free key: https://twelvedata.com/register");
            return;
        }
    };

    let symbol = env_str("AURUM_SYMBOL", "XAU/USD");
    let interval = env::args()
        .nth(1)
        .unwrap_or_else(|| env_str("AURUM_INTERVAL", "15min"));
    let outputsize = env_usize("AURUM_OUTPUTSIZE", 500);
    let window = env_usize("AURUM_WINDOW", 100);
    let htf_n = env_usize("AURUM_HTF_RATIO", 5);
    let min_score = env_usize("AURUM_MIN_SCORE", 6) as u32;

    println!("◈ Aurum · {} · {} · {} bars · window={} · HTF=1:{} · min_score={}",
             symbol, interval, outputsize, window, htf_n, min_score);

    let candles = match fetch_twelvedata(&api_key, &symbol, &interval, outputsize).await {
        Ok(c) => {
            println!("✔ fetched {} real {} candles\n", c.len(), symbol);
            c
        }
        Err(e) => {
            eprintln!("✖ fetch failed: {}", e);
            return;
        }
    };

    if candles.len() < 100 {
        eprintln!("✖ need ≥100 candles (got {}); raise AURUM_OUTPUTSIZE", candles.len());
        return;
    }

    let htf = aggregate(&candles, htf_n);
    let (bias, bias_event) = htf_bias(&htf);

    let ltf_structs = detect_structure(&candles, 2);
    let ltf_sweeps = liquidity_sweeps(&candles, 2);
    let ltf_fvgs = find_fvgs(&candles);
    let ltf_ifvgs = ifvg_entries(&candles);

    let mut signals = Vec::new();
    signals.extend(strategy1(&candles, &htf, htf_n));
    signals.extend(strategy2(&candles, &htf, htf_n));
    signals.extend(strategy3(&candles, &htf, htf_n));
    signals.retain(|s| s.score >= min_score);
    signals.sort_by_key(|s| s.idx);

    // ─── Header ───
    println!("╔══════════════════════════════════════════════════════════════════════╗");
    println!("║  AURUM · {} · {} · {} bars                                  ",
             symbol, interval, candles.len());
    println!("╚══════════════════════════════════════════════════════════════════════╝");

    let bias_line = match bias_event {
        Some(e) => format!("{} (last HTF {} @ bar {})", bias.label(), e.kind.label(), e.idx),
        None => bias.label().to_string(),
    };
    println!(
        "  BIAS: {}{}{}\n",
        fg(if bias == Side::Bull { "bright_green" } else { "bright_red" }),
        bias_line,
        RESET
    );

    // ─── Chart ───
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
    let fresh_fvgs: Vec<_> = ltf_fvgs
        .iter()
        .filter(|f| fvg_status_at(&candles, f, end) != FvgStatus::Mitigated)
        .rev()
        .take(6)
        .collect();
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
            StructureKind::Bos => (
                if ev.direction == Side::Bull { '▲' } else { '▼' },
                "bright_yellow",
            ),
            StructureKind::Choch => ('◆', "bright_yellow"),
            StructureKind::Mss => ('✦', "bright_magenta"),
        };
        chart.draw_marker(ev.idx, ev.level, ch, color);
    }
    for sw in &ltf_sweeps {
        chart.draw_marker(sw.idx, sw.level, '○', "bright_cyan");
    }
    for iv in &ltf_ifvgs {
        chart.draw_marker(iv.idx, iv.entry_price, '◈', "bright_magenta");
    }

    for sig in &signals {
        chart.draw_hband(sig.entry, sig.idx, '═', "bright_green", "");
        chart.draw_hband(sig.stop, sig.idx, '─', "bright_red", "");
        chart.draw_hband(sig.tp1, sig.idx, '─', "bright_yellow", "");
        chart.draw_hband(sig.tp2, sig.idx, '─', "bright_yellow", "");
        chart.draw_hband(sig.tp3, sig.idx, '─', "yellow", "");
    }

    let last = candles[end - 1].close;
    chart.draw_hband(last, end - 1, '▶', "bright_white", "");

    println!("{}", chart.render());

    // ─── Legend ───
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

    // ─── Signals ───
    if signals.is_empty() {
        println!("  No signals above threshold ({}) on this dataset.\n", min_score);
        return;
    }

    println!("  ┌─ SIGNALS ─────────────────────────────────────────────────────────────────────────┐");
    println!(
        "  │ {:<3} {:<18} {:<5} {:>9} {:>9} {:>9} {:>9} {:>9} {:>5} │",
        "#", "STRATEGY", "SIDE", "ENTRY", "STOP", "TP1", "TP2", "TP3", "SCORE"
    );
    println!("  ├───────────────────────────────────────────────────────────────────────────────────┤");
    for (k, s) in signals.iter().enumerate() {
        let color = if s.side == Side::Bull { "bright_green" } else { "bright_red" };
        println!(
            "  │ {:<3} {:<18} {}{:<5}{} {:>9.2} {:>9.2} {:>9.2} {:>9.2} {:>9.2} {:>5} │",
            k + 1, s.strategy, fg(color), s.side.label(), RESET,
            s.entry, s.stop, s.tp1, s.tp2, s.tp3, s.score
        );
        println!(
            "  │     {}reasons: {}{}",
            fg("dim"),
            s.reasons.join(" · "),
            RESET
        );
    }
    println!("  └───────────────────────────────────────────────────────────────────────────────────┘");
    println!("\n  Total: {} signals\n", signals.len());
}