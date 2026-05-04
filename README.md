# SUBS

A Rust CLI tool for analysing StatsBomb open football event data and recommending substitutions based on player fatigue.

Parses match events and team lineups, computes per-player activity decay scores, detects tactical problems, and recommends bench players — via a standard terminal output or a live full-screen dashboard.

---

## Data

Built against the [StatsBomb Open Data](https://github.com/statsbomb/open-data) repository. Clone it into `data/open-data/` relative to the project root:

```bash
git clone https://github.com/statsbomb/open-data data/open-data
```

Expected layout:

```
data/open-data/data/
  events/<match_id>.json
  lineups/<match_id>.json
  matches/<competition_id>/<season_id>.json
```

---

## Build

```bash
cargo build
```

---

## Usage

```bash
cargo run -- --match-id <ID> [--team <TEAM>] [--minute <N>] [--dashboard] [--verbose]
```

| Flag | Default | Description |
|---|---|---|
| `--match-id` | — | StatsBomb match ID (required) |
| `--team` | — | Team name to analyse |
| `--minute` | `90` | Current match minute for decay scoring |
| `--data-dir` | `data/open-data/data` | Path to StatsBomb data directory |
| `--dashboard` | off | Launch full-screen terminal UI (requires `--team`) |
| `--verbose` | off | Print debug info for bench candidate scoring |

---

## Modes

### Standard output

Prints event summary, per-player decay table, and substitution recommendation card:

```bash
cargo run -- --match-id 69225 --team "Barcelona" --minute 60
```

```
Total events loaded: 3379
Home team: Barcelona (23 players)
Away team: Real Madrid (23 players)

PLAYER                 | POS   |  BASELINE APM |  RECENT APM | DECAY
-----------------------------------------------------------------
Sergio Busquets        | CDM   |          0.84 |        0.21 |  0.75
...

═══════════════════════════════════════════════════════
  SUBWATCH RECOMMENDATION — minute 60
═══════════════════════════════════════════════════════
  TACTICAL PROBLEM: Midfield press collapsing

  TAKE OFF:
    1. Sergio Busquets (CDM) — decay 0.75

  BRING ON:
    1. Carles Aleñá (CM) — fit score 2.0  [BEST MATCH]
    2. ...
═══════════════════════════════════════════════════════
```

### Dashboard

Live full-screen terminal UI with real-time minute adjustment:

```bash
cargo run -- --match-id 69225 --team "Barcelona" --minute 60 --dashboard
```

**Layout:**

```
 SUBWATCH • Barcelona vs Real Madrid • Minute: 60
┌ Decay Scores (23) ────────────────────┐┌ Recommendation ──────────────────────┐
│PLAYER                 POS   DEC  BAR  ││TACTICAL PROBLEM: Midfield press...   │
│Sergio Busquets        CDM  0.75 ████░ ││                                      │
│Arturo Vidal           CM   0.61 ███░░ ││TAKE OFF:                             │
│Ivan Rakitic           CM   0.44 ██░░░ ││  1. Sergio Busquets (CDM) — 0.75    │
│...                                    ││                                      │
│                                       ││BRING ON:                             │
│                                       ││  1. Carles Aleñá (CM)  [BEST MATCH] │
└───────────────────────────────────────┘└──────────────────────────────────────┘
 [q] quit  [+] minute+1  [-] minute-1  [r] reload  [↑↓] scroll
```

**Decay bar colours:** red `>0.60` · yellow `0.25–0.60` · green `<0.25`  
**Subbed-off players** are dimmed with a `*` marker.

**Keys:**

| Key | Action |
|---|---|
| `q` | Quit |
| `+` / `=` | Advance minute |
| `-` | Rewind minute |
| `r` | Reload match data |
| `↑` / `↓` | Scroll decay list |

---

## How decay scoring works

For a given `--minute N`:

- **Baseline window:** minutes 0 – min(N/2, 45) — establishes the player's normal activity rate
- **Recent window:** last 15 minutes — measures current output
- **Decay score:** `1 - (recent_APM / baseline_APM)`, clamped to [0, 1]
  - `0.00` = playing at or above baseline (fresh)
  - `1.00` = no recent activity (exhausted)

Players above `0.25` decay trigger a substitution recommendation.

---

## Examples

**La Liga — El Clásico:**
```bash
cargo run -- --match-id 69225 --team "Barcelona" --minute 75 --dashboard
```

**Find match IDs for a competition** (competition 11 = La Liga, season 41):
```bash
python3 -c "
import json
for m in json.load(open('data/open-data/data/matches/11/41.json')):
    print(m['match_id'], m['home_team']['home_team_name'], 'vs', m['away_team']['away_team_name'])
"
```

---

## Project structure

```
src/
  main.rs       # CLI entry point (clap)
  ingest.rs     # StatsBomb JSON parsing, domain structs, position resolution
  decay.rs      # Per-player fatigue scoring
  matcher.rs    # Bench candidate scoring and substitution recommendation
  dashboard.rs  # ratatui full-screen terminal UI
```

### Key types

| Type | Module | Description |
|---|---|---|
| `MatchState` | `ingest` | Home lineup + away lineup + all events |
| `MatchEvent` | `ingest` | Single event: type, player, team, minute, location |
| `LineupPlayer` | `ingest` | Player with name, jersey number, position |
| `DecayScore` | `decay` | Per-player fatigue score with baseline/recent APM |
| `Recommendation` | `matcher` | Tactical problem, take-off list, ranked bench candidates |
