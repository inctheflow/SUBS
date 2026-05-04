# subwatch

A Rust CLI tool for ingesting and analysing StatsBomb open football event data.

Parses match events and team lineups from StatsBomb's JSON format into strongly-typed Rust structs, giving you a clean foundation to build analysis on top of.

---

## Data

This tool is built against the [StatsBomb Open Data](https://github.com/statsbomb/open-data) repository. Clone it into `data/open-data/` relative to the project root:

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
cargo run -- --match-id <ID> [--team <TEAM_NAME>] [--data-dir <PATH>]
```

| Flag | Required | Default | Description |
|---|---|---|---|
| `--match-id` | yes | — | StatsBomb match ID |
| `--team` | no | — | Filter event count by team name |
| `--data-dir` | no | `data/open-data/data` | Path to the StatsBomb data directory |

### Output

```
Total events loaded: 3379
Home team: Barcelona (14 players)
Away team: Real Madrid (14 players)

First 5 events:
  [  0'] Starting XI................... -
  [  0'] Starting XI................... -
  [  0'] Half Start.................... -
  [  0'] Half Start.................... -
  [  0'] Pass.......................... Gonzalo Gerardo Higuaín

Events for 'Barcelona': 2055
```

---

## Examples

**UCL 2018/19 — Tottenham vs Liverpool:**
```bash
cargo run -- --match-id 22912 --team "Liverpool"
```

**La Liga — El Clásico (Barcelona vs Real Madrid):**
```bash
cargo run -- --match-id 69225 --team "Barcelona"
```

**Find match IDs for a competition** (competition 16 = Champions League, season 4 = 2018/19):
```bash
cat data/open-data/data/matches/16/4.json | python3 -c "
import json, sys
for m in json.load(sys.stdin):
    print(m['match_id'], m['home_team']['home_team_name'], 'vs', m['away_team']['away_team_name'])
"
```

---

## Project structure

```
src/
  main.rs      # CLI entry point (clap)
  ingest.rs    # StatsBomb JSON parsing, domain structs
```

### Key types (`ingest.rs`)

| Type | Description |
|---|---|
| `MatchEvent` | Single event: type, player, team, location, minute |
| `LineupPlayer` | Player: name, jersey number, first recorded position |
| `TeamLineup` | Team with full player list |
| `MatchState` | Home lineup + away lineup + all events for a match |

### Public API

```rust
use subwatch::ingest;

let state = ingest::load_match(Path::new("data/open-data/data"), "22912")?;
```
