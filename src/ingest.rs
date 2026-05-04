use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
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

// Public entry point

pub fn load_match(data_dir: &Path, match_id: &str) -> Result<MatchState> {
    let events_path = data_dir.join("events").join(format!("{}.json", match_id));
    let lineups_path = data_dir.join("lineups").join(format!("{}.json", match_id));

    let events = parse_events(&events_path)
        .with_context(|| format!("loading events for match {}", match_id))?;
    let (home_lineup, away_lineup) = parse_lineups(&lineups_path)
        .with_context(|| format!("loading lineups for match {}", match_id))?;

    Ok(MatchState {
        home_lineup,
        away_lineup,
        events,
    })
}
