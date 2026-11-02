# Aurum · XAU/USD ICT Scanner

> A terminal-based gold scanner that detects Smart Money Concepts (ICT) setups in real time and renders them as an ASCII chart with entry, stop, and take-profit levels.

![Aurum Terminal Preview](assets/aurum-snapshot.png)

**Aurum** (Latin for *gold*) analyses live **XAU/USD** candles from [Twelve Data](https://twelvedata.com) and surfaces high-confluence trade setups using three independent strategy engines, each scored by a confluence model before being displayed.

```
╔══════════════════════════════════════════════════════════════════════╗
║  AURUM · XAU/USD · 15min · 500 bars                                  ║
╚══════════════════════════════════════════════════════════════════════╝
  BIAS: LONG (last HTF BOS @ bar 110)

   2684.12 │                        ◆   █  ░░░░░░░░░░░░░░░░░░░░░░░
   2678.44 │                             █
   2672.76 │                   ▲        ██
   2667.08 │                           █
   2661.40 │  ▓▓▓▓ SUPPLY ▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
   2655.72 │                    ○        █
   2650.04 │                            ██
   2644.36 │                          ██
   2638.68 │                      ◆ ██
   2633.00 │══════════════ ENTRY ═══════════════════════════════════
   2627.32 │─────── STOP ───────────────────────────────────────────
   2621.64 │─────── TP1 ────────────────────────────────────────────
   2615.96 │─────── TP2 ────────────────────────────────────────────
   2610.28 │  ░░░ DEMAND ░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░
   2604.60 │              ◈
   2598.92 │            ██
```

---

## Table of contents

- [Features](#features)
- [Prerequisites](#prerequisites)
- [Installation](#installation)
- [Configuration](#configuration)
- [Usage](#usage)
- [Strategies](#strategies)
- [Reading the chart](#reading-the-chart)
- [Confluence scoring](#confluence-scoring)
- [Project structure](#project-structure)
- [Development](#development)
- [Troubleshooting](#troubleshooting)
- [Data source & limits](#data-source--limits)
- [License](#license)

---

## Features

- **Real market data** — pulls live XAU/USD OHLCV candles from Twelve Data (1min → 1day)
- **Three ICT strategy engines** running in parallel:
  - Liquidity Sweep → MSS → Order Block → LTF IFVG
  - Double BOS → Supply/Demand → LTF Breaker Block
  - HTF ChoCh → Unmitigated FVG → LTF IFVG
- **Confluence scoring** (0–12) with reasoning — low-quality setups are filtered out
- **Terminal chart** — candles, supply/demand zones, FVGs, IFVGs, BOS / ChoCh / MSS, sweeps, and entry/SL/TP bands drawn in-place
- **Multi-target exits** — TP1 (1R), TP2 (2R), TP3 (structure-based)
- **ATR-buffered stops** — no fixed pip counts
- **HTF bias + premium/discount filters** — trades aligned with the higher-timeframe narrative only
- **`.env`-driven configuration** — change symbol, timeframe, thresholds without recompiling

---

## Prerequisites

- **Rust 1.75+** — install from [rustup.rs](https://rustup.rs)
- **A free Twelve Data API key** — get one at [twelvedata.com/register](https://twelvedata.com/register) (8 req/min, 800/day, no credit card)
- **Windows / macOS / Linux** — anything Rust supports

---

## Installation

```bash
git clone https://github.com/<you>/aurum.git
cd aurum

# Create the .env file from the template
cp .env.example .env          # Windows PowerShell: Copy-Item .env.example .env

# Edit .env and paste your Twelve Data API key
$EDITOR .env

# Build
cargo build --release
# or: make release
```

The binary lands at `target/release/aurum` (or `aurum.exe` on Windows).

---

## Configuration

All runtime settings live in `.env`:

```dotenv
TWELVE_DATA_API_KEY=your_key_here      # required
AURUM_SYMBOL=XAU/USD                   # instrument
AURUM_INTERVAL=15min                   # candle interval
AURUM_OUTPUTSIZE=500                   # number of candles to fetch
AURUM_WINDOW=100                       # candles shown in chart
AURUM_HTF_RATIO=5                      # LTF → HTF aggregation ratio
AURUM_MIN_SCORE=6                      # minimum confluence score to display
```

**Precedence order** (highest wins):

1. CLI argument — `aurum 1h` overrides interval
2. Process environment — `TWELVE_DATA_API_KEY=abc aurum`
3. `.env` file
4. Built-in default

`.env` is gitignored. `.env.example` is safe to commit.

### Common `AURUM_HTF_RATIO` values

| LTF interval | `AURUM_HTF_RATIO` | Effective HTF |
|---|---|---|
| `5min`  | 3  | 15min |
| `15min` | 4  | 1h    |
| `15min` | 16 | 4h    |
| `1h`    | 4  | 4h    |
| `1h`    | 24 | Daily |

Pick a ratio that puts your HTF at the timeframe you actually use for bias (1h or 4h are the classic choices).

---

## Usage

```bash
# Default run — reads .env
./target/release/aurum

# Override interval without touching .env
./target/release/aurum 1h

# With make
make run
make watch INTERVAL=1h

# Container / CI (no .env file)
TWELVE_DATA_API_KEY=abc123 ./target/release/aurum
```

### Example output

```
◈ Aurum · XAU/USD · 15min · 500 bars · window=100 · HTF=1:5 · min_score=6
✔ fetched 500 real XAU/USD candles

╔══════════════════════════════════════════════════════════════════════╗
║  AURUM · XAU/USD · 15min · 500 bars                                  ║
╚══════════════════════════════════════════════════════════════════════╝
  BIAS: LONG (last HTF BOS @ bar 142)

  [ chart ]

  LEGEND  ▲▼ BOS  ◆ ChoCh  ✦ MSS  ○ Sweep  ◈ IFVG  ░ FVG
  ██ Supply  ██ Demand  ▶ Price  ══ Entry  ── SL  ── TP

  ┌─ SIGNALS ─────────────────────────────────────────────────────────────────────────┐
  │ #   STRATEGY           SIDE      ENTRY      STOP       TP1       TP2  ...  SCORE  │
  ├───────────────────────────────────────────────────────────────────────────────────┤
  │ 1   S1 Sweep+MSS+IFVG  LONG    2684.12   2671.30   2696.94   2709.76      9      │
  │     reasons: sweep · MSS · IFVG · S/D · fresh · HTF-align · vol                   │
  └───────────────────────────────────────────────────────────────────────────────────┘

  Total: 1 signal
```

---

## Strategies

Each strategy follows the same pipeline: **HTF context → POI → LTF trigger → scored entry**.

### 1 · Liquidity Sweep + IFVG

> *"The market hunts liquidity, breaks structure, then retests the order block that caused the move."*

1. HTF **liquidity sweep** (wick through a prior swing, close back inside)
2. HTF **MSS** (market structure shift) in the reversal direction
3. Mark the **order block** that caused the MSS
4. Wait for price to return to the OB
5. LTF **MSS** confirms
6. Enter on the LTF **IFVG** (inverse fair value gap) retest
7. Stop below the OB (bull) / above (bear); targets = 1R / 2R / structure

### 2 · Supply/Demand + Breaker Block

> *"Two consecutive breaks of structure validate a zone; the breaker is where you get filled."*

1. HTF **two consecutive BOS** in the same direction (no ChoCh between)
2. Mark the **supply/demand zone** that preceded the first BOS
3. Wait for price to return to the zone
4. Drop to LTF, look for **MSS**
5. Place a **limit order** at the **breaker block** (the opposite-colour candle just before the MSS)
6. Stop beyond the breaker; targets = 1R / 2R / structure

### 3 · HTF ChoCh + FVG

> *"A ChoCh after a strong trend leaves an unfilled gap — that gap becomes the retest entry."*

1. HTF **strong prior trend** (≥ 2 BOS in the opposite direction)
2. HTF **ChoCh** (first break of structure against the trend)
3. Find the **unmitigated FVG** created by the ChoCh leg
4. Wait for price to return to the FVG
5. LTF **IFVG** confirms
6. Enter on the IFVG; stop beyond the FVG; targets = 1R / 2R / structure

---

## Reading the chart

| Symbol | Meaning | Colour |
|---|---|---|
| `▲` / `▼` | BOS (bullish / bearish) | bright yellow |
| `◆` | ChoCh — change of character | bright yellow |
| `✦` | MSS — ChoCh confirmed by a sweep | bright magenta |
| `○` | Liquidity sweep | bright cyan |
| `◈` | IFVG retest (entry trigger) | bright magenta |
| `░` | Fair value gap (fresh, unfilled) | dim |
| `██` (green) | Demand zone | dark green background |
| `██` (red) | Supply zone | dark red background |
| `██` (blue) | Bullish FVG | dark blue background |
| `██` (purple) | Bearish FVG | dark purple background |
| `██` (orange) | IFVG (flipped gap) | dark orange background |
| `══` | Entry price line | bright green |
| `──` (red) | Stop-loss line | bright red |
| `──` (yellow) | TP1 / TP2 / TP3 lines | bright yellow |
| `▶` | Current close price | bright white |

The **BIAS** header tells you the HTF narrative — LONG, SHORT, or NEUTRAL — along with the last structural event that set it.

---

## Confluence scoring

Every candidate setup is scored from 0 to 12. Only setups with `score ≥ AURUM_MIN_SCORE` (default 6) are displayed.

| Factor | Weight | What it means |
|---|---|---|
| **Sweep** | +2 | Liquidity sweep present on the relevant timeframe |
| **MSS** | +2 | Market structure shift (not a plain BOS) confirmed the reversal |
| **IFVG** | +2 | LTF inverse FVG gave the entry |
| **FVG** | +1 | An unmitigated FVG sits between entry and target |
| **S/D zone** | +1 | Entry is inside a marked supply/demand zone |
| **Fresh** | +1 | The zone / FVG has not been previously mitigated |
| **HTF align** | +1 | Trade direction matches the HTF bias |
| **P/D** | +1 | Entry is in the correct premium/discount half of the recent range |
| **Vol** | +1 | ATR is above the volatility floor (avoids dead markets) |

Raise `AURUM_MIN_SCORE` to **8** for A+ setups only; lower it to **5** in quiet markets if the scanner produces zero signals.

---

## Project structure

```
aurum/
├── Cargo.toml
├── Cargo.lock
├── Makefile
├── README.md
├── .env                    # your key — gitignored
├── .env.example            # template — committed
├── .gitignore
└── src/
    └── main.rs             # everything (single-file binary)
```

Inside `main.rs`, sections are delimited by boxed comments:

- `DATA MODEL` — `Candle`, `Side`, `Zone`, `Signal`, …
- `DETECTION PRIMITIVES` — swings, sweeps, structure, FVGs, IFVGs, order blocks, ATR
- `CONFLUENCE SCORING` — `Factors`, `score()`, `build_signal()`
- `STRATEGY 1/2/3` — three independent scanners
- `TERMINAL CHART` — ASCII renderer
- `DATA FETCHER` — Twelve Data HTTP client
- `MAIN` — wiring

---

## Development

The included `Makefile` wraps common commands:

| Command | Action |
|---|---|
| `make help` | List targets |
| `make setup` | Create `.env` from `.env.example` |
| `make build` | Debug build |
| `make release` | Optimized build → `target/release/aurum` |
| `make run` | Run with `.env` defaults |
| `make watch INTERVAL=1h` | Run with a custom interval |
| `make check` | Type-check only |
| `make fmt` | `cargo fmt` |
| `make lint` | `cargo clippy -- -D warnings` |
| `make test` | `cargo test` |
| `make clean` | Remove `target/` |

---

## Troubleshooting

### `can't find bin aurum at path src/main.rs`

Cargo can't find `src/main.rs`. Confirm:

```powershell
# Windows
Get-ChildItem -Recurse -Depth 1 -Include *.rs, *.toml
```

`main.rs` must be at `aurum\src\main.rs`, not `aurum\main.rs`.

### `TWELVE_DATA_API_KEY missing or placeholder`

You haven't set your key. Run `make setup`, edit `.env`, and paste your key from [twelvedata.com/register](https://twelvedata.com/register).

### `Twelve Data API error: ...`

The API returned an error message. Common causes:

| Message | Cause | Fix |
|---|---|---|
| `apikey is invalid` | Typo or expired key | Re-copy from your dashboard |
| `You have reached the limit of requests` | 8 req/min or 800/day exceeded | Wait, or upgrade plan |
| `symbol XAU/USD not available` | Your plan doesn't include XAU/USD | Free tier *does* include it — check the key is on the right account |
| `interval is invalid` | Misspelled interval | Use `1min`, `5min`, `15min`, `30min`, `1h`, `4h`, `1day` |

### `need ≥100 candles (got N)`

Your fetch returned fewer candles than the analysis requires. Raise `AURUM_OUTPUTSIZE` or use a larger interval.

### Chart looks garbled

Your terminal needs **256-color ANSI** support and a font with box-drawing characters (`█`, `░`, `│`, `─`). On Windows, use **Windows Terminal** or **VS Code's integrated terminal** — the old `cmd.exe` console misrenders colors.

---

## Data source & limits

- **Provider:** Twelve Data — [twelvedata.com](https://twelvedata.com)
- **Endpoint:** `GET /time_series?symbol=XAU/USD&interval=...&outputsize=...`
- **Free tier:** 8 requests/minute, 800 requests/day, no credit card
- **Data coverage:** XAU/USD spot, ~20 years of history, 1min → 1day bars
- **Response order:** newest-first → the fetcher reverses to chronological
- **Authentication:** API key only (public market data — no signing, no secrets in headers)

The scanner makes **exactly one request per run**, so you can safely run it several times per minute without hitting the rate limit.

---

## License

MIT — see [`LICENSE`](LICENSE) if present, or add one before publishing.

---

## Disclaimer

Trading gold (XAU/USD) on leverage carries substantial risk of loss. This tool is for **educational and research purposes only**. Signals are algorithmic outputs, not financial advice. Past performance of any strategy shown here does not guarantee future results. Never risk capital you cannot afford to lose.
