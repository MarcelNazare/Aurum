# Aurum · XAU/USD ICT Scanner

> A high-performance Smart Money Concepts (ICT) scanner for gold (XAU/USD), available as a native desktop GUI and a local Web UI.

![Aurum Terminal Preview](assets/aurum-snapshot.png)

**Aurum** (Latin for *gold*) analyses live **XAU/USD** candles from [Twelve Data](https://twelvedata.com) and surfaces high-confluence trade setups using three independent strategy engines. Every setup is scored by a confluence model and enriched with live status tracking and Estimated Time of Arrival (ETA) calculations.

Ships as **two binaries** sharing a single Rust engine:

| Binary | Interface | Use case |
|---|---|---|
| `aurum-gui` | Native Desktop (egui) | Fastest performance, dedicated app window |
| `aurum-web` | Local Web UI (Canvas) | Interactive charting, accessible from any browser |

---

## Table of Contents

- [Features](#features)
- [Prerequisites](#prerequisites)
- [Installation](#installation)
- [Configuration & API Keys](#configuration--api-keys)
- [Usage — Native GUI](#usage--native-gui)
- [Usage — Web UI](#usage--web-ui)
- [Signal Metadata](#signal-metadata)
- [Architecture](#architecture)
- [Strategies](#strategies)
- [Reading the Chart](#reading-the-chart)
- [Confluence Scoring](#confluence-scoring)
- [Project Structure](#project-structure)
- [Development](#development)
- [Troubleshooting](#troubleshooting)
- [Data Source & Limits](#data-source--limits)
- [License](#license)

---

## Features

- **Real Market Data** — Live XAU/USD OHLCV from Twelve Data (1min to 1day intervals).
- **Three ICT Strategy Engines** — Sweep+MSS+IFVG, SD+Breaker, and ChoCh+FVG.
- **Advanced Signal Tracking**:
  - **Status**: Tracks if a setup is `WAITING`, `ACTIVE`, `INVALIDATED`, or `COMPLETED`.
  - **ETA**: Calculates the estimated time to reach the entry based on recent volatility (ATR).
  - **Classification**: Distinguishes between `Trend` and `Counter-Trend` setups.
- **Deduplication Engine** — Automatically filters out overlapping signals sharing the exact same entry and stop levels.
- **Multi-Target Exits** — TP1 (1R), TP2 (2R), and TP3 (3R) with ATR-buffered stop-losses.
- **BYOK (Bring Your Own Key)** — Custom API key fields in both UIs, saved locally for convenience.
- **Interactive Web Chart** — Hover over candles, zones, and signal lines to inspect exact data. Scroll to zoom, drag to pan.

---

## Prerequisites

- **Rust 1.75+** — Install from [rustup.rs](https://rustup.rs).
- **A free Twelve Data API key** — Get one at [twelvedata.com/register](https://twelvedata.com/register) (8 req/min, 800/day).
- **Windows, macOS, or Linux**.
- A modern web browser (for the Web UI).

---

## Installation

```bash
git clone https://github.com/yourusername/aurum.git
cd aurum

# Optional: Copy environment variables template
cp .env.example .env

# Build both binaries in release mode
cargo build --release
```

The resulting binaries will be available at:
- `target/release/aurum-gui`
- `target/release/aurum-web`

*(Note: On Windows, append `.exe` to the filenames).*

---

## Configuration & API Keys

Aurum requires a Twelve Data API key. You can provide this in three ways (ordered by priority):

### 1. In-App Input (Recommended & Saved)
Both the GUI and Web UI feature an **API Key** input field.
- **Web UI**: Saves your key to the browser's `localStorage` so it persists across page reloads.
- **Native GUI**: Saves your key to a local file named `aurum_api_key.txt` in the same directory as the executable.

### 2. Environment Variables (`.env`)
If you prefer not to use the UI fields, you can provide the key via a `.env` file:

```dotenv
TWELVE_DATA_API_KEY=your_api_key_here
AURUM_SYMBOL=XAU/USD
AURUM_INTERVAL=15min
AURUM_OUTPUTSIZE=500
AURUM_WINDOW=100
AURUM_HTF_RATIO=5
AURUM_MIN_SCORE=6
AURUM_PORT=3000
```

### 3. Process Environment
```bash
TWELVE_DATA_API_KEY=abc123 ./target/release/aurum-gui
```

---

## Usage — Native GUI

Launch the dedicated desktop application:

```bash
./target/release/aurum-gui
```

**How to use:**
1. Enter your API key in the top bar (it will save automatically).
2. Adjust your Interval, Bars, and other settings.
3. Click **Fetch & Analyze**.
4. Hover over the chart to see detailed tooltips. Scroll to zoom, drag to pan.
5. View detailed signals in the table at the bottom.

---

## Usage — Web UI

Launch the local web server (this will automatically open your default browser):

```bash
./target/release/aurum-web
```

**Options:**
```bash
# Run on a custom port
./target/release/aurum-web --port 8080

# Run headless (do not auto-open browser)
./target/release/aurum-web --no-open
```

**How to use:**
1. Enter your API key in the top-left field. It is saved in your browser for next time.
2. Adjust your settings and click **Fetch & Analyze**.
3. Hover over any chart element for exact prices, timestamps, and statuses.

---

## Signal Metadata

Every generated signal includes several advanced data points to help you manage the trade:

- **Type**:
  - `Trend`: The setup aligns with the Higher Time Frame (HTF) bias.
  - `Counter`: The setup goes against the HTF bias.
- **Status**:
  - `WAITING`: Price has not yet reached the entry level.
  - `ACTIVE`: Price has touched the entry and the trade is live.
  - `INVALIDATED`: Price hit the stop-loss before reaching the entry (setup is dead).
  - `COMPLETED`: Price hit TP3.
- **ETA (Estimated Time of Arrival)**:
  - For `WAITING` signals, calculates how long it should take for price to reach the entry based on the last 20 candles of volatility.
  - For `ACTIVE` signals, displays `Hit`.

---

## Architecture

```text
┌──────────────────────────────────────────────────────────────┐
│                                                              │
│  aurum-gui binary             aurum-web binary               │
│  ─────────────────            ──────────────────             │
│  egui native window           axum HTTP server               │
│  (Instant rendering)          (GET /, GET /api/analyze)      │
│         │                              │                     │
│         └──────────────┬───────────────┘                     │
│                        ▼                                     │
│              aurum library (src/lib.rs)                      │
│              ─────────────────────────                       │
│              • Candle, Signal, Zone, Fvg types               │
│              • swings, structure, FVG, IFVG, order blocks    │
│              • strategy1, strategy2, strategy3               │
│              • deduplicate, evaluate, enrich (ETA)           │
│              • fetch_twelvedata (HTTP client)                │
│                                                              │
└──────────────────────────────────────────────────────────────┘
                         │
                         ▼
              Twelve Data API
              (key in .env, file, or browser storage)
```

---

## Strategies

### 1. Liquidity Sweep + IFVG
1. HTF liquidity sweep.
2. HTF Market Structure Shift (MSS).
3. Mark the Order Block (OB) that caused the MSS.
4. Wait for price to return to the OB.
5. LTF MSS confirms.
6. Enter on the LTF Inverse Fair Value Gap (IFVG) retest.
7. Stop below/above the OB; targets at 1R, 2R, 3R.

### 2. Supply/Demand + Breaker Block
1. HTF two consecutive Breaks of Structure (BOS).
2. Mark the Supply/Demand zone that preceded the first BOS.
3. Wait for price to return to the zone.
4. Drop to LTF, look for an MSS.
5. Place an entry at the Breaker Block.
6. Stop beyond the breaker; targets at 1R, 2R, 3R.

### 3. HTF ChoCh + FVG
1. HTF strong prior trend (at least 2 BOS).
2. HTF Change of Character (ChoCh).
3. Find the unmitigated FVG created by the ChoCh leg.
4. Wait for price to return to the FVG.
5. LTF IFVG confirms.
6. Enter on the IFVG; stop beyond the FVG; targets at 1R, 2R, 3R.

---

## Reading the Chart

| Symbol | Meaning | Color |
|---|---|---|
| `▲` / `▼` | BOS (Bullish / Bearish) | Yellow |
| `◆` | ChoCh | Yellow |
| `✦` | MSS (ChoCh confirmed by a sweep) | Magenta |
| `○` | Liquidity Sweep | Cyan |
| `◆` | IFVG Retest | Magenta |
| `░` | Fair Value Gap (FVG) | Blue/Purple |
| `██` | Supply / Demand Zone | Red / Green Background |
| `══` | Entry Line | Bright Green |
| `──` | Stop-Loss Line | Bright Red |
| `──` | Take-Profit Lines | Yellow |

*Note: Invalidated or Completed signals are drawn with 30% opacity so they fade into the background.*

---

## Confluence Scoring

Every setup is scored from 0 to 12. Only setups with a score greater than or equal to `AURUM_MIN_SCORE` (default 6) are displayed.

| Factor | Weight | Description |
|---|---|---|
| **Sweep** | +2 | Liquidity sweep present. |
| **MSS** | +2 | Market Structure Shift confirmed. |
| **IFVG** | +2 | Inverse Fair Value Gap gave the entry. |
| **FVG** | +1 | Unmitigated FVG sits between entry and target. |
| **S/D** | +1 | Entry is inside a supply/demand zone. |
| **Fresh** | +1 | Zone/FVG has not been previously mitigated. |
| **HTF Align** | +1 | Trade direction matches the HTF bias (Trend). |
| **P/D** | +1 | Entry is in the correct Premium/Discount half. |
| **Vol** | +1 | ATR is above the volatility floor. |

---

## Project Structure

```text
aurum/
├── Cargo.toml
├── Makefile
├── README.md
├── .env.example
├── index.html              # Embedded by aurum-web
└── src/
    ├── lib.rs              # Shared engine (strategies, math, types)
    ├── server.rs           # axum web server
    └── bin/
        ├── aurum-gui.rs    # Native desktop UI (egui)
        └── aurum-web.rs    # Web server launcher
```

---

## Development

The included `Makefile` wraps common commands:

| Command | Action |
|---|---|
| `make help` | List targets |
| `make setup` | Create `.env` from `.env.example` |
| `make build` | Debug build (both binaries) |
| `make release` | Optimized build |
| `make gui` | Run the native desktop app |
| `make web` | Run the web UI (auto-opens browser) |
| `make web-headless` | Run the web UI without opening a browser |
| `make check` | Type-check all targets |
| `make lint` | `cargo clippy` |
| `make clean` | Remove `target/` |

---

## Troubleshooting

### `TWELVE_DATA_API_KEY missing or placeholder`
You haven't provided an API key. Enter it directly in the UI or add it to your `.env` file.

### Web UI says "Cannot reach the Aurum server"
Ensure the `aurum-web` process is running in your terminal. If you changed the port, make sure you are opening the correct URL (e.g., `http://localhost:8080`).

### `Twelve Data API error: ...`
- `apikey is invalid`: Double-check your key.
- `You have reached the limit of requests`: You exceeded the free tier (8 req/min). Wait a minute or upgrade your Twelve Data plan.
- `symbol XAU/USD not available`: Ensure your API key has access to Forex/Metals data (free tier usually does).

### Port already in use
```bash
# Run on a different port
./target/release/aurum-web --port 8080
```

---

## Data Source & Limits

- **Provider:** Twelve Data.
- **Free Tier Limits:** 8 requests per minute, 800 requests per day.
- **Authentication:** API key only.

Aurum makes exactly **one request per analysis run**. Running both the GUI and Web UI simultaneously will count as two requests.

---

## License

MIT License. See `LICENSE` for details.

---

## Disclaimer

Trading gold (XAU/USD) on leverage carries substantial risk of loss. This tool is for educational and research purposes only. Signals are algorithmic outputs, not financial advice. Never risk capital you cannot afford to lose.
