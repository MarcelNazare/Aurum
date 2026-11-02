#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::HashSet;
use std::sync::mpsc;
use std::thread;

use chrono::{DateTime, Utc};
use dotenvy::dotenv;
use eframe::egui;
use egui::{Color32, FontId, Pos2, Rect, Rounding, Stroke, Vec2};

use aurum::{
    aggregate, detect_structure, fetch_twelvedata, find_fvgs, fvg_status_at, htf_bias,
    ifvg_entries, liquidity_sweeps, order_block_before, strategy1, strategy2, strategy3,
    Candle, Fvg, FvgStatus, IfvgEntry, Side, Signal, StructureEvent, StructureKind, Sweep, Zone,
};

// ═══════════════════════════════════════════════════════════════════════════
// PALETTE
// ═══════════════════════════════════════════════════════════════════════════

const COL_BG:      Color32 = Color32::from_rgb(0x0b, 0x0d, 0x10);
const COL_BG_SOFT: Color32 = Color32::from_rgb(0x14, 0x17, 0x1c);
const COL_BORDER:  Color32 = Color32::from_rgb(0x1f, 0x24, 0x2b);
const COL_TEXT:    Color32 = Color32::from_rgb(0xe5, 0xe7, 0xeb);
const COL_DIM:     Color32 = Color32::from_rgb(0x6b, 0x72, 0x80);
const COL_GREEN:   Color32 = Color32::from_rgb(0x22, 0xc5, 0x5e);
const COL_RED:     Color32 = Color32::from_rgb(0xef, 0x44, 0x44);
const COL_GOLD:    Color32 = Color32::from_rgb(0xd4, 0xaf, 0x37);
const COL_CYAN:    Color32 = Color32::from_rgb(0x22, 0xd3, 0xee);
const COL_MAGENTA: Color32 = Color32::from_rgb(0xd9, 0x46, 0xef);
const COL_AMBER:   Color32 = Color32::from_rgb(0xea, 0xb3, 0x08);

const GRID_LINE:   Color32 = Color32::from_rgba_premultiplied(0xff, 0xff, 0xff, 15);
const CROSSHAIR:   Color32 = Color32::from_rgba_premultiplied(0xd4, 0xaf, 0x37, 96);

fn zone_color(side: Side, hover: bool) -> (Color32, Color32) {
    match (side, hover) {
        (Side::Bull, false) => (Color32::from_rgba_premultiplied(0x22, 0xc5, 0x5e,  26), Color32::from_rgba_premultiplied(0x22, 0xc5, 0x5e, 90)),
        (Side::Bull, true)  => (Color32::from_rgba_premultiplied(0x22, 0xc5, 0x5e,  60), Color32::from_rgba_premultiplied(0x22, 0xc5, 0x5e, 230)),
        (Side::Bear, false) => (Color32::from_rgba_premultiplied(0xef, 0x44, 0x44,  26), Color32::from_rgba_premultiplied(0xef, 0x44, 0x44, 90)),
        (Side::Bear, true)  => (Color32::from_rgba_premultiplied(0xef, 0x44, 0x44,  60), Color32::from_rgba_premultiplied(0xef, 0x44, 0x44, 230)),
    }
}
fn fvg_color(side: Side) -> Color32 {
    match side {
        Side::Bull => Color32::from_rgba_premultiplied(0x3b, 0x82, 0xf6, 26),
        Side::Bear => Color32::from_rgba_premultiplied(0xa8, 0x55, 0xf7, 26),
    }
}
fn ifvg_color(hover: bool) -> Color32 {
    if hover { Color32::from_rgba_premultiplied(0xea, 0xb3, 0x08, 60) }
    else     { Color32::from_rgba_premultiplied(0xea, 0xb3, 0x08, 22) }
}

// ═══════════════════════════════════════════════════════════════════════════
// TYPES
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Clone, Copy, PartialEq)]
enum HoverKind {
    Candle(usize),
    Structure(usize),
    Sweep(usize),
    Ifvg(usize),
    Zone(usize),
    Signal(usize),
}

#[derive(Clone, Copy)]
enum StatusKind { Info, Success, Error }

struct AnalysisData {
    symbol: String,
    interval: String,
    candles: Vec<Candle>,
    bias: Side,
    bias_kind: Option<StructureKind>,
    bias_idx: Option<usize>,
    structures: Vec<StructureEvent>,
    sweeps: Vec<Sweep>,
    fvgs: Vec<Fvg>,
    ifvgs: Vec<IfvgEntry>,
    zones: Vec<Zone>,
    signals: Vec<Signal>,
}

struct Layout {
    inner: Rect,
    start: usize,
    end: usize,
    lo: f64,
    hi: f64,
}

impl Layout {
    fn x_of(&self, idx: usize) -> f32 {
        if self.end <= self.start { return self.inner.left(); }
        self.inner.left()
            + (idx as f32 - self.start as f32)
                / (self.end - self.start) as f32
                * self.inner.width()
    }
    fn y_of(&self, price: f64) -> f32 {
        self.inner.top()
            + ((self.hi - price) / (self.hi - self.lo).max(1e-9)) as f32
                * self.inner.height()
    }
    fn idx_at(&self, x: f32) -> usize {
        let frac = (x - self.inner.left()) / self.inner.width().max(1.0);
        self.start + (frac * (self.end - self.start) as f32).round() as usize
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// ANALYSIS (runs on a background thread)
// ═══════════════════════════════════════════════════════════════════════════

async fn run_analysis(
    api_key: String,
    symbol: String,
    interval: String,
    outputsize: usize,
    htf_ratio: usize,
    min_score: u32,
) -> Result<AnalysisData, String> {
    let candles = fetch_twelvedata(&api_key, &symbol, &interval, outputsize)
        .await
        .map_err(|e| e.to_string())?;

    if candles.len() < 100 {
        return Err(format!(
            "only {} candles returned; increase outputsize or use a larger interval",
            candles.len()
        ));
    }

    let htf = aggregate(&candles, htf_ratio);
    let (bias_dir, bias_event) = htf_bias(&htf);
    let structures = detect_structure(&candles, 2);
    let sweeps = liquidity_sweeps(&candles, 2);
    let all_ifvgs = ifvg_entries(&candles);

    let mut fresh_fvgs: Vec<Fvg> = find_fvgs(&candles)
        .into_iter()
        .filter(|f| fvg_status_at(&candles, f, candles.len()) == FvgStatus::Fresh)
        .collect();
    if fresh_fvgs.len() > 8 {
        fresh_fvgs = fresh_fvgs.split_off(fresh_fvgs.len() - 8);
    }

    let mut recent_ifvgs = all_ifvgs;
    if recent_ifvgs.len() > 6 {
        recent_ifvgs = recent_ifvgs.split_off(recent_ifvgs.len() - 6);
    }

    let mut signals = Vec::new();
    signals.extend(strategy1(&candles, &htf, htf_ratio));
    signals.extend(strategy2(&candles, &htf, htf_ratio));
    signals.extend(strategy3(&candles, &htf, htf_ratio));
    signals.retain(|s| s.score >= min_score);
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

    let (bias_kind, bias_idx) = match bias_event {
        Some(e) => (Some(e.kind), Some(e.idx)),
        None => (None, None),
    };

    Ok(AnalysisData {
        symbol,
        interval,
        candles,
        bias: bias_dir,
        bias_kind,
        bias_idx,
        structures,
        sweeps,
        fvgs: fresh_fvgs,
        ifvgs: recent_ifvgs,
        zones,
        signals,
    })
}

// ═══════════════════════════════════════════════════════════════════════════
// APP
// ═══════════════════════════════════════════════════════════════════════════

struct AurumApp {
    api_key: String,
    symbol: String,

    interval: String,
    outputsize: usize,
    window: usize,
    htf_ratio: usize,
    min_score: u32,

    data: Option<AnalysisData>,
    loading: bool,
    rx: Option<mpsc::Receiver<Result<AnalysisData, String>>>,
    status: (String, StatusKind),

    zoom: usize,
    offset: usize,
    hover: Option<HoverKind>,
}

impl AurumApp {
    fn new(
        api_key: String,
        symbol: String,
        interval: String,
        outputsize: usize,
        window: usize,
        htf_ratio: usize,
        min_score: u32,
    ) -> Self {
        let status = if api_key.is_empty() || api_key == "your_api_key_here" {
            ("Set TWELVE_DATA_API_KEY in .env, then click Fetch & Analyze.".to_string(), StatusKind::Error)
        } else {
            ("Ready. Click Fetch & Analyze.".to_string(), StatusKind::Info)
        };
        Self {
            api_key,
            symbol,
            interval,
            outputsize,
            window,
            htf_ratio,
            min_score,
            data: None,
            loading: false,
            rx: None,
            status,
            zoom: window,
            offset: 0,
            hover: None,
        }
    }

    fn start_fetch(&mut self) {
        if self.loading { return; }
        if self.api_key.is_empty() || self.api_key == "your_api_key_here" {
            self.status = (
                "TWELVE_DATA_API_KEY missing or placeholder — edit .env first.".into(),
                StatusKind::Error,
            );
            return;
        }
        self.loading = true;
        self.status = (
            format!("Fetching {} × {} candles for {}…", self.outputsize, self.interval, self.symbol),
            StatusKind::Info,
        );
        self.hover = None;

        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);

        let api_key = self.api_key.clone();
        let symbol = self.symbol.clone();
        let interval = self.interval.clone();
        let outputsize = self.outputsize;
        let htf_ratio = self.htf_ratio;
        let min_score = self.min_score;

        thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("tokio runtime");
            let result = rt.block_on(run_analysis(
                api_key, symbol, interval, outputsize, htf_ratio, min_score,
            ));
            let _ = tx.send(result);
        });
    }

    fn poll_result(&mut self) {
        let Some(rx) = &self.rx else { return; };
        match rx.try_recv() {
            Ok(Ok(data)) => {
                let n = data.signals.len();
                self.data = Some(data);
                self.loading = false;
                self.rx = None;
                self.zoom = self.window;
                self.offset = 0;
                self.status = (
                    format!("✔ Done — {} signal{}", n, if n == 1 { "" } else { "s" }),
                    StatusKind::Success,
                );
            }
            Ok(Err(e)) => {
                self.loading = false;
                self.rx = None;
                self.status = (format!("✖ {}", e), StatusKind::Error);
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => {
                self.loading = false;
                self.rx = None;
                self.status = ("✖ fetch thread crashed".into(), StatusKind::Error);
            }
        }
    }
}

impl eframe::App for AurumApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_result();
        if self.loading {
            ctx.request_repaint_after(std::time::Duration::from_millis(80));
        }

        // ── Controls ──
        let mut do_fetch = false;
        let mut do_reset = false;
        egui::TopBottomPanel::top("controls")
            .frame(egui::Frame { fill: COL_BG_SOFT, ..Default::default() })
            .show(ctx, |ui| {
                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    ui.heading(egui::RichText::new("AURUM").color(COL_GOLD).strong());
                    ui.label(egui::RichText::new(format!("{} · ICT Scanner", self.symbol)).color(COL_DIM));
                    ui.separator();

                    ui.label("Interval:");
                    egui::ComboBox::from_id_source("interval")
                        .selected_text(&self.interval)
                        .show_ui(ui, |ui| {
                            for iv in ["1min", "5min", "15min", "30min", "1h", "4h", "1day"] {
                                ui.selectable_value(&mut self.interval, iv.to_string(), iv);
                            }
                        });
                    ui.label("Bars:");
                    ui.add(egui::DragValue::new(&mut self.outputsize).range(100..=5000).speed(10));
                    ui.label("Window:");
                    ui.add(egui::DragValue::new(&mut self.window).range(20..=500).speed(1));
                    ui.label("HTF:");
                    ui.add(egui::DragValue::new(&mut self.htf_ratio).range(2..=50));
                    ui.label("Min score:");
                    ui.add(egui::DragValue::new(&mut self.min_score).range(0..=12));
                    ui.separator();

                    if ui.add_enabled(!self.loading, egui::Button::new("Fetch & Analyze")).clicked() {
                        do_fetch = true;
                    }
                    if ui.button("Reset view").clicked() {
                        do_reset = true;
                    }
                });
                ui.add_space(4.0);
            });

        // ── Status ──
        if !self.status.0.is_empty() {
            egui::TopBottomPanel::top("status")
                .frame(egui::Frame { fill: COL_BG, ..Default::default() })
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        if self.loading { ui.spinner(); }
                        let col = match self.status.1 {
                            StatusKind::Info => COL_CYAN,
                            StatusKind::Success => COL_GREEN,
                            StatusKind::Error => COL_RED,
                        };
                        ui.colored_label(col, &self.status.0);
                    });
                });
        }

        // ── Bias bar ──
        if let Some(data) = &self.data {
            egui::TopBottomPanel::top("bias")
                .frame(egui::Frame { fill: COL_BG_SOFT, ..Default::default() })
                .show(ctx, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(egui::RichText::new(&data.symbol).color(COL_GOLD).strong());
                        ui.label(egui::RichText::new(&data.interval).color(COL_DIM));
                        ui.separator();
                        ui.label("BIAS:");
                        let (txt, col) = match data.bias {
                            Side::Bull => ("LONG", COL_GREEN),
                            Side::Bear => ("SHORT", COL_RED),
                        };
                        ui.colored_label(col, egui::RichText::new(txt).strong());
                        if let (Some(kind), Some(idx)) = (data.bias_kind, data.bias_idx) {
                            let kind_str = match kind {
                                StructureKind::Bos => "BOS",
                                StructureKind::Choch => "ChoCh",
                                StructureKind::Mss => "MSS",
                            };
                            ui.label(format!("· last HTF {} @ bar {}", kind_str, idx));
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(
                                egui::RichText::new(format!("{} candles", data.candles.len()))
                                    .color(COL_DIM),
                            );
                        });
                    });
                });
        }

        // ── Signals table ──
        if self.data.is_some() {
            egui::TopBottomPanel::bottom("signals")
                .resizable(true)
                .default_height(220.0)
                .frame(egui::Frame { fill: COL_BG_SOFT, ..Default::default() })
                .show(ctx, |ui| {
                    let data = self.data.as_ref().unwrap();
                    signals_ui(ui, data);
                });
        }

        // ── Chart ──
        egui::CentralPanel::default()
            .frame(egui::Frame { fill: COL_BG, ..Default::default() })
            .show(ctx, |ui| {
                chart_ui(
                    ui,
                    self.data.as_ref(),
                    &mut self.zoom,
                    &mut self.offset,
                    &mut self.hover,
                    self.window,
                );
            });

        if do_fetch { self.start_fetch(); }
        if do_reset { self.zoom = self.window; self.offset = 0; }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// CHART
// ═══════════════════════════════════════════════════════════════════════════

fn chart_ui(
    ui: &mut egui::Ui,
    data: Option<&AnalysisData>,
    zoom: &mut usize,
    offset: &mut usize,
    hover: &mut Option<HoverKind>,
    window: usize,
) {
    let avail = ui.available_rect_before_wrap();
    let response = ui.allocate_rect(avail, egui::Sense::click_and_drag());

    let painter = ui.painter_at(avail);
    painter.rect_filled(avail, Rounding::ZERO, COL_BG);

    let Some(data) = data else {
        painter.text(
            avail.center(),
            egui::Align2::CENTER_CENTER,
            "Click Fetch & Analyze to start",
            FontId::monospace(14.0),
            COL_DIM,
        );
        *hover = None;
        return;
    };

    let layout = match compute_layout(data, avail, *zoom, *offset) {
        Some(l) => l,
        None => return,
    };

    // Wheel zoom
    if response.hovered() {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll.abs() > 0.1 {
            let new_zoom = if scroll > 0.0 {
                (*zoom as f32 * 1.1).round() as usize
            } else {
                (*zoom as f32 / 1.1).round() as usize
            };
            *zoom = new_zoom.clamp(20, 500);
        }
    }

    // Drag pan
    if response.dragged() {
        let dx = response.drag_delta().x;
        let bars_per_px = (layout.end - layout.start) as f32 / layout.inner.width().max(1.0);
        let shift = (dx * bars_per_px).round() as i32;
        let new_off = *offset as i32 - shift;
        let max_off = data.candles.len().saturating_sub(*zoom) as i32;
        *offset = new_off.clamp(0, max_off.max(0)) as usize;
    }

    // Recompute after potential zoom/pan
    let layout = match compute_layout(data, avail, *zoom, *offset) {
        Some(l) => l,
        None => return,
    };

    // Hover
    let hover_pos = response.hover_pos();
    let current_hover = hover_pos.and_then(|p| hit_test(&layout, p, data));
    *hover = current_hover;

    // Draw everything
    draw_grid(&painter, &layout, data);
    draw_zones(&painter, &layout, data, current_hover);
    draw_fvgs(&painter, &layout, data, current_hover);
    draw_candles(&painter, &layout, data, current_hover);
    draw_structures(&painter, &layout, data, current_hover);
    draw_sweeps(&painter, &layout, data, current_hover);
    draw_ifvg_markers(&painter, &layout, data, current_hover);
    draw_signal_lines(&painter, &layout, data, current_hover);
    draw_current_price(&painter, &layout, data);

    // Crosshair
    if let Some(pos) = hover_pos {
        if layout.inner.contains(pos) {
            draw_crosshair(&painter, &layout, pos);
        }
    }

    // Tooltip
    if let (Some(_pos), Some(h)) = (hover_pos, current_hover) {
        if !response.dragged() {
            egui::show_tooltip_at_pointer(
                ui.ctx(),
                ui.layer_id(),
                egui::Id::new("chart_tip"),
                |ui| render_tooltip(ui, &layout, data, h),
            );
        }
    }

    // Info line at the top of the chart
    let info = format!(
        "{} of {} candles · {} → {}",
        layout.end - layout.start,
        data.candles.len(),
        fmt_time_short(data.candles[layout.start.min(data.candles.len() - 1)].ts, &data.interval),
        fmt_time_short(data.candles[(layout.end - 1).min(data.candles.len() - 1)].ts, &data.interval),
    );
    painter.text(
        Pos2::new(avail.left() + 66.0, avail.top() + 6.0),
        egui::Align2::LEFT_TOP,
        info,
        FontId::monospace(10.0),
        COL_DIM,
    );
    painter.text(
        Pos2::new(avail.right() - 74.0, avail.top() + 6.0),
        egui::Align2::RIGHT_TOP,
        format!("window={} · zoom with wheel · drag to pan", window),
        FontId::monospace(10.0),
        COL_DIM,
    );
}

fn compute_layout(data: &AnalysisData, rect: Rect, zoom: usize, offset: usize) -> Option<Layout> {
    let pad_l = 66.0_f32;
    let pad_r = 74.0_f32;
    let pad_t = 24.0_f32;
    let pad_b = 30.0_f32;
    let inner = Rect::from_min_max(
        Pos2::new(rect.left() + pad_l, rect.top() + pad_t),
        Pos2::new(rect.right() - pad_r, rect.bottom() - pad_b),
    );
    if inner.width() < 40.0 || inner.height() < 40.0 {
        return None;
    }

    let total = data.candles.len();
    if total == 0 { return None; }

    let zoom = zoom.max(20).min(total);
    let end = total.saturating_sub(offset).max(1);
    let start = end.saturating_sub(zoom);
    if start >= end { return None; }

    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for i in start..end {
        if let Some(c) = data.candles.get(i) {
            lo = lo.min(c.low);
            hi = hi.max(c.high);
        }
    }
    for z in &data.zones {
        if z.start_idx < end {
            lo = lo.min(z.bottom);
            hi = hi.max(z.top);
        }
    }
    for f in &data.fvgs {
        if f.start_idx < end {
            lo = lo.min(f.bottom);
            hi = hi.max(f.top);
        }
    }
    for s in &data.signals {
        if s.idx >= start && s.idx < end {
            lo = lo.min(s.stop).min(s.entry).min(s.tp1).min(s.tp2).min(s.tp3);
            hi = hi.max(s.stop).max(s.entry).max(s.tp1).max(s.tp2).max(s.tp3);
        }
    }
    if !lo.is_finite() || !hi.is_finite() {
        return None;
    }
    let m = (hi - lo) * 0.05;
    let m = if m.abs() < 1e-9 { 1.0 } else { m };
    lo -= m;
    hi += m;

    Some(Layout { inner, start, end, lo, hi })
}

fn draw_grid(painter: &egui::Painter, layout: &Layout, data: &AnalysisData) {
    let font = FontId::monospace(10.0);
    for i in 0..=6 {
        let p = layout.lo + (layout.hi - layout.lo) * i as f64 / 6.0;
        let y = layout.y_of(p);
        painter.line_segment(
            [Pos2::new(layout.inner.left(), y), Pos2::new(layout.inner.right(), y)],
            Stroke::new(1.0_f32, GRID_LINE),
        );
        painter.text(
            Pos2::new(layout.inner.left() - 8.0, y),
            egui::Align2::RIGHT_CENTER,
            format!("{:.2}", p),
            font.clone(),
            COL_DIM,
        );
    }
    for i in 0..=6 {
        let idx = layout.start + (layout.end - layout.start) * i / 6;
        if idx >= layout.end || idx >= data.candles.len() { continue; }
        let x = layout.x_of(idx);
        painter.text(
            Pos2::new(x, layout.inner.bottom() + 6.0),
            egui::Align2::CENTER_TOP,
            fmt_time_short(data.candles[idx].ts, &data.interval),
            FontId::monospace(9.0),
            COL_DIM,
        );
    }
}

fn draw_zones(painter: &egui::Painter, layout: &Layout, data: &AnalysisData, hover: Option<HoverKind>) {
    for (i, z) in data.zones.iter().enumerate() {
        let x0 = layout.x_of(z.start_idx.max(layout.start));
        if x0 >= layout.inner.right() { continue; }
        let yt = layout.y_of(z.top);
        let yb = layout.y_of(z.bottom);
        let is_hover = hover == Some(HoverKind::Zone(i));
        let (fill, border) = zone_color(z.side, is_hover);
        let rect = Rect::from_min_max(
            Pos2::new(x0, yt.min(yb)),
            Pos2::new(layout.inner.right(), yt.max(yb)),
        );
        painter.rect_filled(rect, Rounding::ZERO, fill);
        painter.rect_stroke(
            rect,
            Rounding::ZERO,
            Stroke::new(if is_hover { 1.5_f32 } else { 1.0_f32 }, border),
        );
    }
}

fn draw_fvgs(painter: &egui::Painter, layout: &Layout, data: &AnalysisData, hover: Option<HoverKind>) {
    for f in &data.fvgs {
        let x0 = layout.x_of(f.start_idx.max(layout.start));
        if x0 >= layout.inner.right() { continue; }
        let yt = layout.y_of(f.top);
        let yb = layout.y_of(f.bottom);
        let rect = Rect::from_min_max(
            Pos2::new(x0, yt.min(yb)),
            Pos2::new(layout.inner.right(), yt.max(yb)),
        );
        painter.rect_filled(rect, Rounding::ZERO, fvg_color(f.side));
    }
    for (i, iv) in data.ifvgs.iter().enumerate() {
        let f = iv.fvg;
        let x0 = layout.x_of(f.start_idx.max(layout.start));
        if x0 >= layout.inner.right() { continue; }
        let yt = layout.y_of(f.top);
        let yb = layout.y_of(f.bottom);
        let is_hover = hover == Some(HoverKind::Ifvg(i));
        let rect = Rect::from_min_max(
            Pos2::new(x0, yt.min(yb)),
            Pos2::new(layout.inner.right(), yt.max(yb)),
        );
        painter.rect_filled(rect, Rounding::ZERO, ifvg_color(is_hover));
    }
}

fn draw_candles(painter: &egui::Painter, layout: &Layout, data: &AnalysisData, hover: Option<HoverKind>) {
    let total = layout.end - layout.start;
    let cw = layout.inner.width() / total.max(1) as f32;
    let bw = (cw * 0.7).max(1.0);

    for i in 0..total {
        let idx = layout.start + i;
        let Some(c) = data.candles.get(idx) else { continue };
        let x = layout.x_of(idx) + cw * 0.5;
        let up = c.close >= c.open;
        let color = if up { COL_GREEN } else { COL_RED };
        let is_hover = hover == Some(HoverKind::Candle(idx));

        let yh = layout.y_of(c.high);
        let yl = layout.y_of(c.low);
        painter.line_segment(
            [Pos2::new(x, yh), Pos2::new(x, yl)],
            Stroke::new(if is_hover { 2.0_f32 } else { 1.0_f32 }, color),
        );

        let yo = layout.y_of(c.open);
        let yc = layout.y_of(c.close);
        let top = yo.min(yc);
        let bot = yo.max(yc);
        let h = (bot - top).max(1.0);
        painter.rect_filled(
            Rect::from_min_size(Pos2::new(x - bw * 0.5, top), Vec2::new(bw, h)),
            Rounding::ZERO,
            color,
        );

        if is_hover {
            painter.rect_stroke(
                Rect::from_min_size(
                    Pos2::new(x - bw * 0.5 - 1.0, top - 1.0),
                    Vec2::new(bw + 2.0, h + 2.0),
                ),
                Rounding::ZERO,
                Stroke::new(1.2_f32, COL_GOLD),
            );
        }
    }
}

fn diamond_points(x: f32, y: f32, r: f32) -> Vec<Pos2> {
    vec![
        Pos2::new(x, y - r),
        Pos2::new(x + r, y),
        Pos2::new(x, y + r),
        Pos2::new(x - r, y),
    ]
}

fn star_points(x: f32, y: f32, r: f32) -> Vec<Pos2> {
    let mut pts = Vec::with_capacity(10);
    for i in 0..10 {
        let a = -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::PI / 5.0;
        let rr = if i % 2 == 0 { r } else { r * 0.4 };
        pts.push(Pos2::new(x + a.cos() * rr, y + a.sin() * rr));
    }
    pts
}

fn draw_structures(painter: &egui::Painter, layout: &Layout, data: &AnalysisData, hover: Option<HoverKind>) {
    for (i, ev) in data.structures.iter().enumerate() {
        if ev.idx < layout.start || ev.idx >= layout.end { continue; }
        let x = layout.x_of(ev.idx);
        let y = layout.y_of(ev.level);
        let is_hover = hover == Some(HoverKind::Structure(i));

        if is_hover {
            painter.circle_filled(Pos2::new(x, y), 10.0, Color32::from_rgba_premultiplied(0xff, 0xff, 0xff, 38));
        }

        let r = if is_hover { 7.0 } else { 5.0 };
        match ev.kind {
            StructureKind::Bos => {
                let up = ev.direction == Side::Bull;
                let pts = if up {
                    vec![Pos2::new(x, y - r), Pos2::new(x - r * 0.87, y + r * 0.5), Pos2::new(x + r * 0.87, y + r * 0.5)]
                } else {
                    vec![Pos2::new(x, y + r), Pos2::new(x - r * 0.87, y - r * 0.5), Pos2::new(x + r * 0.87, y - r * 0.5)]
                };
                painter.add(egui::Shape::convex_polygon(pts, COL_AMBER, Stroke::NONE));
            }
            StructureKind::Choch => {
                painter.add(egui::Shape::convex_polygon(diamond_points(x, y, r), COL_AMBER, Stroke::NONE));
            }
            StructureKind::Mss => {
                painter.add(egui::Shape::convex_polygon(star_points(x, y, r + 1.0), COL_MAGENTA, Stroke::NONE));
            }
        }
    }
}

fn draw_sweeps(painter: &egui::Painter, layout: &Layout, data: &AnalysisData, hover: Option<HoverKind>) {
    for (i, sw) in data.sweeps.iter().enumerate() {
        if sw.idx < layout.start || sw.idx >= layout.end { continue; }
        let x = layout.x_of(sw.idx);
        let y = layout.y_of(sw.level);
        let is_hover = hover == Some(HoverKind::Sweep(i));
        let r = if is_hover { 7.0 } else { 4.5 };
        if is_hover {
            painter.circle_filled(Pos2::new(x, y), r, Color32::from_rgba_premultiplied(0x22, 0xd3, 0xee, 60));
        }
        painter.circle_stroke(
            Pos2::new(x, y),
            r,
            Stroke::new(if is_hover { 2.2_f32 } else { 1.5_f32 }, COL_CYAN),
        );
    }
}

fn draw_ifvg_markers(painter: &egui::Painter, layout: &Layout, data: &AnalysisData, hover: Option<HoverKind>) {
    for (i, iv) in data.ifvgs.iter().enumerate() {
        if iv.idx < layout.start || iv.idx >= layout.end { continue; }
        let x = layout.x_of(iv.idx);
        let y = layout.y_of(iv.entry_price);
        let is_hover = hover == Some(HoverKind::Ifvg(i));
        let r = if is_hover { 7.0 } else { 4.5 };
        painter.add(egui::Shape::convex_polygon(diamond_points(x, y, r), COL_MAGENTA, Stroke::NONE));
    }
}

fn draw_signal_lines(painter: &egui::Painter, layout: &Layout, data: &AnalysisData, hover: Option<HoverKind>) {
    let font = FontId::monospace(9.0);
    for (i, sig) in data.signals.iter().enumerate() {
        if sig.idx < layout.start || sig.idx >= layout.end { continue; }
        let is_hover = hover == Some(HoverKind::Signal(i));
        let x0 = layout.x_of(sig.idx);
        let x1 = layout.inner.right();

        let ew: f32 = if is_hover { 2.2 } else { 1.4 };
        let sw: f32 = if is_hover { 2.0 } else { 1.2 };

        painter.line_segment(
            [Pos2::new(x0, layout.y_of(sig.entry)), Pos2::new(x1, layout.y_of(sig.entry))],
            Stroke::new(ew, COL_GREEN),
        );
        painter.line_segment(
            [Pos2::new(x0, layout.y_of(sig.stop)), Pos2::new(x1, layout.y_of(sig.stop))],
            Stroke::new(sw, COL_RED),
        );
        for tp in [sig.tp1, sig.tp2, sig.tp3] {
            painter.line_segment(
                [Pos2::new(x0, layout.y_of(tp)), Pos2::new(x1, layout.y_of(tp))],
                Stroke::new(if is_hover { 1.8_f32 } else { 1.0_f32 }, COL_AMBER),
            );
        }

        let lx = layout.inner.right() + 3.0;
        painter.text(Pos2::new(lx, layout.y_of(sig.entry)), egui::Align2::LEFT_CENTER, "E",  font.clone(), COL_GREEN);
        painter.text(Pos2::new(lx, layout.y_of(sig.stop)),  egui::Align2::LEFT_CENTER, "SL", font.clone(), COL_RED);
        painter.text(Pos2::new(lx, layout.y_of(sig.tp1)),   egui::Align2::LEFT_CENTER, "T1", font.clone(), COL_AMBER);
        painter.text(Pos2::new(lx, layout.y_of(sig.tp2)),   egui::Align2::LEFT_CENTER, "T2", font.clone(), COL_AMBER);
        painter.text(Pos2::new(lx, layout.y_of(sig.tp3)),   egui::Align2::LEFT_CENTER, "T3", font.clone(), Color32::from_rgb(0xd9, 0x77, 0x06));

        // Signal bar marker
        let color = if sig.side == Side::Bull { COL_GREEN } else { COL_RED };
        let y = layout.y_of(sig.entry);
        let pts = if sig.side == Side::Bull {
            vec![Pos2::new(x0 - 5.0, y - 5.0), Pos2::new(x0 + 5.0, y - 5.0), Pos2::new(x0, y + 4.0)]
        } else {
            vec![Pos2::new(x0 - 5.0, y + 5.0), Pos2::new(x0 + 5.0, y + 5.0), Pos2::new(x0, y - 4.0)]
        };
        painter.add(egui::Shape::convex_polygon(pts, color, Stroke::NONE));
    }
}

fn draw_current_price(painter: &egui::Painter, layout: &Layout, data: &AnalysisData) {
    let Some(last) = data.candles.last() else { return; };
    if last.close < layout.lo || last.close > layout.hi { return; }
    let y = layout.y_of(last.close);
    painter.line_segment(
        [Pos2::new(layout.inner.left(), y), Pos2::new(layout.inner.right(), y)],
        Stroke::new(1.0_f32, Color32::from_rgba_premultiplied(0xff, 0xff, 0xff, 140)),
    );
    let text = format!("{:.2}", last.close);
    let pw = 58.0_f32;
    let ph = 16.0_f32;
    let rect = Rect::from_min_size(
        Pos2::new(layout.inner.right() + 2.0, y - ph * 0.5),
        Vec2::new(pw, ph),
    );
    painter.rect_filled(rect, Rounding::ZERO, Color32::WHITE);
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        text,
        FontId::monospace(10.0),
        COL_BG,
    );
}

fn draw_crosshair(painter: &egui::Painter, layout: &Layout, pos: Pos2) {
    if !layout.inner.contains(pos) { return; }
    painter.line_segment(
        [Pos2::new(pos.x, layout.inner.top()), Pos2::new(pos.x, layout.inner.bottom())],
        Stroke::new(1.0_f32, CROSSHAIR),
    );
    painter.line_segment(
        [Pos2::new(layout.inner.left(), pos.y), Pos2::new(layout.inner.right(), pos.y)],
        Stroke::new(1.0_f32, CROSSHAIR),
    );
    let price = layout.hi
        - (pos.y - layout.inner.top()) as f64
            / layout.inner.height().max(1.0) as f64
            * (layout.hi - layout.lo);
    let text = format!("{:.2}", price);
    let rect = Rect::from_min_size(
        Pos2::new(layout.inner.right() + 2.0, pos.y - 9.0),
        Vec2::new(58.0, 18.0),
    );
    painter.rect_filled(rect, Rounding::ZERO, Color32::from_rgb(0x1a, 0x1d, 0x23));
    painter.rect_stroke(rect, Rounding::ZERO, Stroke::new(1.0_f32, COL_GOLD));
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        text,
        FontId::monospace(10.0),
        COL_GOLD,
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// HIT TEST
// ═══════════════════════════════════════════════════════════════════════════

fn hit_test(layout: &Layout, pos: Pos2, data: &AnalysisData) -> Option<HoverKind> {
    if !layout.inner.contains(pos) { return None; }

    for (i, sig) in data.signals.iter().enumerate() {
        if sig.idx < layout.start || sig.idx >= layout.end { continue; }
        if pos.x < layout.x_of(sig.idx) { continue; }
        for p in [sig.entry, sig.stop, sig.tp1, sig.tp2, sig.tp3] {
            if (pos.y - layout.y_of(p)).abs() < 6.0 {
                return Some(HoverKind::Signal(i));
            }
        }
    }

    for (i, ev) in data.structures.iter().enumerate() {
        if ev.idx < layout.start || ev.idx >= layout.end { continue; }
        if (pos.x - layout.x_of(ev.idx)).hypot(pos.y - layout.y_of(ev.level)) < 10.0 {
            return Some(HoverKind::Structure(i));
        }
    }
    for (i, sw) in data.sweeps.iter().enumerate() {
        if sw.idx < layout.start || sw.idx >= layout.end { continue; }
        if (pos.x - layout.x_of(sw.idx)).hypot(pos.y - layout.y_of(sw.level)) < 9.0 {
            return Some(HoverKind::Sweep(i));
        }
    }
    for (i, iv) in data.ifvgs.iter().enumerate() {
        if iv.idx < layout.start || iv.idx >= layout.end { continue; }
        if (pos.x - layout.x_of(iv.idx)).hypot(pos.y - layout.y_of(iv.entry_price)) < 9.0 {
            return Some(HoverKind::Ifvg(i));
        }
    }

    for (i, z) in data.zones.iter().enumerate() {
        let x0 = layout.x_of(z.start_idx.max(layout.start));
        if x0 >= layout.inner.right() { continue; }
        if pos.x < x0 { continue; }
        let yt = layout.y_of(z.top);
        let yb = layout.y_of(z.bottom);
        if pos.y >= yt.min(yb) && pos.y <= yt.max(yb) {
            return Some(HoverKind::Zone(i));
        }
    }

    let idx = layout.idx_at(pos.x);
    if idx >= layout.start && idx < layout.end && idx < data.candles.len() {
        return Some(HoverKind::Candle(idx));
    }
    None
}

// ═══════════════════════════════════════════════════════════════════════════
// TOOLTIP
// ═══════════════════════════════════════════════════════════════════════════

fn render_tooltip(ui: &mut egui::Ui, layout: &Layout, data: &AnalysisData, h: HoverKind) {
    let _ = layout;
    ui.style_mut().override_font_id = Some(FontId::monospace(12.0));

    match h {
        HoverKind::Candle(idx) => {
            let Some(c) = data.candles.get(idx) else { return };
            ui.strong(fmt_time_full(c.ts, &data.interval));
            ui.separator();
            let up = c.close >= c.open;
            let col = if up { COL_GREEN } else { COL_RED };
            ui.horizontal(|ui| { ui.label("Open:");  ui.colored_label(col, format!("{:.2}", c.open)); });
            ui.horizontal(|ui| { ui.label("High:");  ui.colored_label(col, format!("{:.2}", c.high)); });
            ui.horizontal(|ui| { ui.label("Low:");   ui.colored_label(col, format!("{:.2}", c.low)); });
            ui.horizontal(|ui| { ui.label("Close:"); ui.colored_label(col, format!("{:.2}", c.close)); });
            let chg = (c.close - c.open) / c.open * 100.0;
            ui.colored_label(col, format!("{:+.2}%  · bar #{}", chg, idx));
        }
        HoverKind::Structure(i) => {
            let Some(ev) = data.structures.get(i) else { return };
            let kind = match ev.kind {
                StructureKind::Bos => "BOS",
                StructureKind::Choch => "ChoCh",
                StructureKind::Mss => "MSS",
            };
            let dir = if ev.direction == Side::Bull { "BULLISH" } else { "BEARISH" };
            let col = if ev.direction == Side::Bull { COL_GREEN } else { COL_RED };
            ui.strong(format!("{} · {}", kind, dir));
            ui.separator();
            ui.horizontal(|ui| { ui.label("Level:"); ui.colored_label(col, format!("{:.2}", ev.level)); });
            ui.horizontal(|ui| { ui.label("Time:");  ui.label(fmt_time_full(data.candles[ev.idx].ts, &data.interval)); });
        }
        HoverKind::Sweep(i) => {
            let Some(sw) = data.sweeps.get(i) else { return };
            let dir = if sw.direction == Side::Bull { "BULLISH" } else { "BEARISH" };
            let col = if sw.direction == Side::Bull { COL_GREEN } else { COL_RED };
            ui.strong(format!("Liquidity Sweep · {}", dir));
            ui.separator();
            ui.horizontal(|ui| { ui.label("Level:"); ui.colored_label(col, format!("{:.2}", sw.level)); });
            ui.horizontal(|ui| { ui.label("Time:");  ui.label(fmt_time_full(data.candles[sw.idx].ts, &data.interval)); });
        }
        HoverKind::Ifvg(i) => {
            let Some(iv) = data.ifvgs.get(i) else { return };
            let dir = if iv.side == Side::Bull { "BULLISH" } else { "BEARISH" };
            let col = if iv.side == Side::Bull { COL_GREEN } else { COL_RED };
            ui.strong(format!("Inverse FVG · {}", dir));
            ui.separator();
            ui.horizontal(|ui| { ui.label("Entry:"); ui.colored_label(col, format!("{:.2}", iv.entry_price)); });
            ui.horizontal(|ui| { ui.label("Gap top:");    ui.label(format!("{:.2}", iv.fvg.top)); });
            ui.horizontal(|ui| { ui.label("Gap bottom:"); ui.label(format!("{:.2}", iv.fvg.bottom)); });
            ui.horizontal(|ui| { ui.label("Time:"); ui.label(fmt_time_full(data.candles[iv.idx].ts, &data.interval)); });
        }
        HoverKind::Zone(i) => {
            let Some(z) = data.zones.get(i) else { return };
            let col = if z.side == Side::Bull { COL_GREEN } else { COL_RED };
            ui.strong(format!("{} Zone", z.label));
            ui.separator();
            ui.horizontal(|ui| { ui.label("Top:");    ui.colored_label(col, format!("{:.2}", z.top)); });
            ui.horizontal(|ui| { ui.label("Bottom:"); ui.colored_label(col, format!("{:.2}", z.bottom)); });
            ui.horizontal(|ui| { ui.label("Height:"); ui.label(format!("{:.2}", z.top - z.bottom)); });
            ui.horizontal(|ui| { ui.label("Start:");  ui.label(fmt_time_full(data.candles[z.start_idx].ts, &data.interval)); });
        }
        HoverKind::Signal(i) => {
            let Some(s) = data.signals.get(i) else { return };
            let col = if s.side == Side::Bull { COL_GREEN } else { COL_RED };
            ui.strong(s.strategy);
            ui.separator();
            ui.horizontal(|ui| { ui.label("Side:"); ui.colored_label(col, if s.side == Side::Bull { "LONG" } else { "SHORT" }); });
            ui.horizontal(|ui| { ui.label("Entry:"); ui.label(format!("{:.2}", s.entry)); });
            ui.horizontal(|ui| { ui.label("Stop:");  ui.colored_label(COL_RED, format!("{:.2}", s.stop)); });
            ui.horizontal(|ui| { ui.label("TP1:");   ui.label(format!("{:.2}", s.tp1)); });
            ui.horizontal(|ui| { ui.label("TP2:");   ui.label(format!("{:.2}", s.tp2)); });
            ui.horizontal(|ui| { ui.label("TP3:");   ui.label(format!("{:.2}", s.tp3)); });
            ui.horizontal(|ui| { ui.label("Score:"); ui.colored_label(COL_GOLD, format!("{}/12", s.score)); });
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                for r in &s.reasons {
                    ui.label(egui::RichText::new(r).small().color(COL_DIM));
                }
            });
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// SIGNALS TABLE
// ═══════════════════════════════════════════════════════════════════════════

fn signals_ui(ui: &mut egui::Ui, data: &AnalysisData) {
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.heading(egui::RichText::new("Signals").color(COL_DIM).size(14.0));
        ui.label(egui::RichText::new(format!("({})", data.signals.len())).color(COL_DIM));
    });

    if data.signals.is_empty() {
        ui.label(egui::RichText::new("No signals above threshold on this dataset.").color(COL_DIM));
        return;
    }

    egui::ScrollArea::vertical().show(ui, |ui| {
        egui::Grid::new("signals_grid")
            .striped(true)
            .spacing([14.0, 4.0])
            .min_col_width(60.0)
            .show(ui, |ui| {
                for h in ["#", "Strategy", "Side", "Entry", "Stop", "TP1", "TP2", "TP3", "Score", "Reasons"] {
                    ui.label(egui::RichText::new(h).strong().color(COL_DIM));
                }
                ui.end_row();

                for (i, s) in data.signals.iter().enumerate() {
                    ui.label(format!("{}", i + 1));
                    ui.label(s.strategy);
                    let (txt, col) = match s.side {
                        Side::Bull => ("LONG", COL_GREEN),
                        Side::Bear => ("SHORT", COL_RED),
                    };
                    ui.colored_label(col, egui::RichText::new(txt).strong());
                    ui.monospace(format!("{:.2}", s.entry));
                    ui.monospace(format!("{:.2}", s.stop));
                    ui.monospace(format!("{:.2}", s.tp1));
                    ui.monospace(format!("{:.2}", s.tp2));
                    ui.monospace(format!("{:.2}", s.tp3));
                    ui.colored_label(
                        if s.score >= 9 { COL_GOLD } else { COL_TEXT },
                        format!("{}/12", s.score),
                    );
                    ui.horizontal_wrapped(|ui| {
                        for r in &s.reasons {
                            ui.label(egui::RichText::new(r).small().color(COL_DIM));
                        }
                    });
                    ui.end_row();
                }
            });
    });
}

// ═══════════════════════════════════════════════════════════════════════════
// TIME FORMATTING
// ═══════════════════════════════════════════════════════════════════════════

fn fmt_time_short(ts: i64, interval: &str) -> String {
    match DateTime::<Utc>::from_timestamp(ts, 0) {
        Some(d) => {
            if interval == "1day" { d.format("%d/%m").to_string() }
            else                  { d.format("%H:%M").to_string() }
        }
        None => String::new(),
    }
}
fn fmt_time_full(ts: i64, interval: &str) -> String {
    match DateTime::<Utc>::from_timestamp(ts, 0) {
        Some(d) => {
            if interval == "1day" { d.format("%d %b %Y").to_string() }
            else                  { d.format("%Y-%m-%d %H:%M UTC").to_string() }
        }
        None => String::new(),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// MAIN
// ═══════════════════════════════════════════════════════════════════════════

fn env_or<T: std::str::FromStr>(key: &str, default: T) -> T {
    std::env::var(key).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

fn main() -> eframe::Result<()> {
    dotenv().ok();

    let api_key = std::env::var("TWELVE_DATA_API_KEY").unwrap_or_default();
    let symbol = std::env::var("AURUM_SYMBOL").unwrap_or_else(|_| "XAU/USD".into());
    let interval = std::env::var("AURUM_INTERVAL").unwrap_or_else(|_| "15min".into());
    let outputsize: usize = env_or("AURUM_OUTPUTSIZE", 500);
    let window: usize = env_or("AURUM_WINDOW", 100);
    let htf_ratio: usize = env_or("AURUM_HTF_RATIO", 5);
    let min_score: u32 = env_or("AURUM_MIN_SCORE", 6);

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1400.0_f32, 900.0_f32])
            .with_min_inner_size([900.0_f32, 600.0_f32])
            .with_title("AURUM · XAU/USD ICT Scanner"),
        ..Default::default()
    };

    eframe::run_native(
        "AURUM",
        options,
        Box::new(move |cc| {
            let mut style = (*cc.egui_ctx.style()).clone();
            style.visuals = egui::Visuals::dark();
            style.visuals.panel_fill = COL_BG;
            style.visuals.window_fill = COL_BG_SOFT;
            style.visuals.extreme_bg_color = COL_BG;
            style.visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, COL_BORDER);
            style.visuals.widgets.inactive.bg_stroke        = Stroke::new(1.0_f32, COL_BORDER);
            style.visuals.widgets.hovered.bg_stroke         = Stroke::new(1.0_f32, COL_GOLD);
            style.visuals.widgets.active.bg_stroke          = Stroke::new(1.0_f32, COL_GOLD);
            cc.egui_ctx.set_style(style);
            Ok(Box::new(AurumApp::new(
                api_key, symbol, interval, outputsize, window, htf_ratio, min_score,
            )))
        }),
    )
}