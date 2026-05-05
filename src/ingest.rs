use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;

// Intermediate structs for deserializing the deeply-nested StatsBomb JSON

#[derive(Deserialize)]
struct NamedObject {
    name: String,
}

#[derive(Deserialize)]
struct PlayerRef {
    // id: u32,
    name: String,
}

#[derive(Deserialize)]
struct RawSubstitution {
    #[allow(dead_code)]
    outcome: NamedObject,
    replacement: PlayerRef,
}

#[derive(Deserialize)]
struct RawDuel {
    outcome: Option<NamedObject>,
}

#[derive(Deserialize)]
struct RawEvent {
    id: String,
    index: u32,
    period: u8,
    timestamp: String,
    minute: u32,
    second: u32,
    #[serde(rename = "type")]
    event_type: NamedObject,
    team: NamedObject,
    player: Option<PlayerRef>,
    location: Option<[f64; 2]>,
    under_pressure: Option<bool>,
    substitution: Option<RawSubstitution>,
    duel: Option<RawDuel>,
}

// Lineup raw structs

#[derive(Deserialize)]
struct RawPosition {
    position: String,
}

#[derive(Deserialize)]
struct RawLineupPlayer {
    player_id: u32,
    player_name: String,
    jersey_number: u8,
    positions: Vec<RawPosition>,
}

#[derive(Deserialize)]
struct RawTeamLineup {
    team_id: u32,
    team_name: String,
    lineup: Vec<RawLineupPlayer>,
}

// Minimal structs for cross-match position index scanning

#[derive(Deserialize)]
struct IdxPlayer {
    player_name: String,
    positions: Vec<RawPosition>,
}

#[derive(Deserialize)]
struct IdxTeam {
    lineup: Vec<IdxPlayer>,
}

// Minimal structs for scanning the matches index

#[derive(Deserialize)]
struct RawMatchHomeTeam {
    home_team_name: String,
}

#[derive(Deserialize)]
struct RawMatchAwayTeam {
    away_team_name: String,
}

#[derive(Deserialize)]
struct RawMatchIndex {
    match_id: u32,
    home_team: RawMatchHomeTeam,
    away_team: RawMatchAwayTeam,
}

// Public domain structs

#[derive(Debug, Serialize)]
pub struct MatchEvent {
    pub id: String,
    pub index: u32,
    pub period: u8,
    pub timestamp: String,
    pub minute: u32,
    pub second: u32,
    pub event_type: String,
    pub player_name: Option<String>,
    pub team_name: String,
    pub location: Option<[f64; 2]>,
    pub under_pressure: Option<bool>,
    // name of the player coming ON (populated for Substitution events only)
    pub substitution_player_on: Option<String>,
    // outcome of the duel (e.g. "Won", "Lost") for Duel events
    pub duel_outcome: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct LineupPlayer {
    pub player_id: u32,
    pub player_name: String,
    pub position: String,
    pub jersey_number: u8,
}

#[derive(Debug, Serialize)]
pub struct TeamLineup {
    pub team_id: u32,
    pub team_name: String,
    pub players: Vec<LineupPlayer>,
}

#[derive(Debug, Serialize)]
pub struct MatchState {
    pub home_lineup: TeamLineup,
    pub away_lineup: TeamLineup,
    pub events: Vec<MatchEvent>,
}

// Parsing helpers

fn parse_events(path: &Path) -> Result<Vec<MatchEvent>> {
    let data = std::fs::read_to_string(path)
        .with_context(|| format!("reading events file: {}", path.display()))?;
    let raw: Vec<RawEvent> =
        serde_json::from_str(&data).context("deserializing events JSON")?;

    Ok(raw
        .into_iter()
        .map(|e| MatchEvent {
            id: e.id,
            index: e.index,
            period: e.period,
            timestamp: e.timestamp,
            minute: e.minute,
            second: e.second,
            event_type: e.event_type.name,
            player_name: e.player.map(|p| p.name),
            team_name: e.team.name,
            location: e.location,
            under_pressure: e.under_pressure,
            substitution_player_on: e.substitution.map(|s| s.replacement.name),
            duel_outcome: e.duel.and_then(|d| d.outcome).map(|o| o.name),
        })
        .collect())
}

fn parse_lineup(raw: RawTeamLineup) -> TeamLineup {
    let players = raw
        .lineup
        .into_iter()
        .map(|p| LineupPlayer {
            player_id: p.player_id,
            player_name: p.player_name,
            position: p
                .positions
                .into_iter()
                .next()
                .map(|pos| pos.position)
                .unwrap_or_else(|| "Unknown".to_string()),
            jersey_number: p.jersey_number,
        })
        .collect();

    TeamLineup {
        team_id: raw.team_id,
        team_name: raw.team_name,
        players,
    }
}

fn parse_lineups(path: &Path) -> Result<(TeamLineup, TeamLineup)> {
    let data = std::fs::read_to_string(path)
        .with_context(|| format!("reading lineups file: {}", path.display()))?;
    let mut raw: Vec<RawTeamLineup> =
        serde_json::from_str(&data).context("deserializing lineups JSON")?;

    anyhow::ensure!(raw.len() >= 2, "expected 2 teams in lineups, got {}", raw.len());

    // StatsBomb lineups: index 0 = home, index 1 = away (order matches match metadata)
    let away = parse_lineup(raw.remove(1));
    let home = parse_lineup(raw.remove(0));
    Ok((home, away))
}

// Scan other lineup files to resolve positions for bench players who have no
// position data in this match's lineup (positions array is empty in StatsBomb
// when a player never came on). Stops as soon as all unknowns are resolved.
fn resolve_positions(
    lineups_dir: &Path,
    skip_match_id: &str,
    unknowns: &HashSet<String>,
) -> HashMap<String, String> {
    let mut resolved: HashMap<String, String> = HashMap::new();
    if unknowns.is_empty() {
        return resolved;
    }

    let Ok(entries) = std::fs::read_dir(lineups_dir) else {
        return resolved;
    };

    for entry in entries.flatten() {
        if resolved.len() == unknowns.len() {
            break;
        }
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        if path.file_stem().and_then(|s| s.to_str()) == Some(skip_match_id) {
            continue;
        }
        let Ok(data) = std::fs::read_to_string(&path) else {
            continue;
        };
        // Fast pre-check: skip JSON parse entirely if no unknown name appears in the file
        if !unknowns.iter().any(|name| data.contains(name.as_str())) {
            continue;
        }
        let Ok(teams) = serde_json::from_str::<Vec<IdxTeam>>(&data) else {
            continue;
        };
        for team in &teams {
            for player in &team.lineup {
                if unknowns.contains(&player.player_name)
                    && !resolved.contains_key(&player.player_name)
                {
                    if let Some(pos) = player.positions.first() {
                        resolved.insert(player.player_name.clone(), pos.position.clone());
                    }
                }
            }
        }
    }

    resolved
}

// Public entry point — events only (no lineup resolution, used for season baseline)

pub fn load_events_only(data_dir: &Path, match_id: &str) -> Result<Vec<MatchEvent>> {
    let events_path = data_dir.join("events").join(format!("{}.json", match_id));
    parse_events(&events_path)
}

// Scan the matches index to find all match IDs where `team` played, excluding `exclude_match_id`.
pub fn find_team_match_ids(data_dir: &Path, team: &str, exclude_match_id: &str) -> Vec<String> {
    let matches_dir = data_dir.join("matches");
    let mut ids = Vec::new();

    let Ok(competitions) = std::fs::read_dir(&matches_dir) else {
        return ids;
    };

    for comp_entry in competitions.flatten() {
        let comp_path = comp_entry.path();
        if !comp_path.is_dir() {
            continue;
        }
        let Ok(seasons) = std::fs::read_dir(&comp_path) else {
            continue;
        };
        for season_entry in seasons.flatten() {
            let path = season_entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let Ok(data) = std::fs::read_to_string(&path) else {
                continue;
            };
            if !data.contains(team) {
                continue;
            }
            let Ok(matches) = serde_json::from_str::<Vec<RawMatchIndex>>(&data) else {
                continue;
            };
            for m in matches {
                let mid = m.match_id.to_string();
                if mid == exclude_match_id {
                    continue;
                }
                if m.home_team.home_team_name.eq_ignore_ascii_case(team)
                    || m.away_team.away_team_name.eq_ignore_ascii_case(team)
                {
                    ids.push(mid);
                }
            }
        }
    }
    ids
}

pub fn load_match(data_dir: &Path, match_id: &str) -> Result<MatchState> {
    let events_path = data_dir.join("events").join(format!("{}.json", match_id));
    let lineups_path = data_dir.join("lineups").join(format!("{}.json", match_id));

    let events = parse_events(&events_path)
        .with_context(|| format!("loading events for match {}", match_id))?;
    let (mut home_lineup, mut away_lineup) = parse_lineups(&lineups_path)
        .with_context(|| format!("loading lineups for match {}", match_id))?;

    // Bench players who never came on have empty positions arrays in the match
    // lineup JSON. Look them up in other matches in the dataset.
    let unknowns: HashSet<String> = home_lineup
        .players
        .iter()
        .chain(away_lineup.players.iter())
        .filter(|p| p.position == "Unknown")
        .map(|p| p.player_name.clone())
        .collect();

    if !unknowns.is_empty() {
        let lineups_dir = data_dir.join("lineups");
        let index = resolve_positions(&lineups_dir, match_id, &unknowns);
        for player in home_lineup
            .players
            .iter_mut()
            .chain(away_lineup.players.iter_mut())
        {
            if player.position == "Unknown" {
                if let Some(pos) = index.get(&player.player_name) {
                    player.position = pos.clone();
                }
            }
        }
    }

    Ok(MatchState {
        home_lineup,
        away_lineup,
        events,
    })
}
