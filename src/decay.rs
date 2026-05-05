use crate::ingest::{MatchState, TeamLineup};
use std::collections::HashMap;
use std::path::Path;

const ACTIVE_TYPES: &[&str] = &[
    "Pass",
    "Carry",
    "Pressure",
    "Duel",
    "Ball Receipt*",
    "Interception",
    "Shot",
    "Dribble",
];

#[derive(Debug)]
pub struct DecayScore {
    pub player_name: String,
    pub position: String,      // abbreviated, for display
    pub position_full: String, // full StatsBomb string, for group matching
    pub baseline_apm: f64,
    pub recent_apm: f64,
    pub score: f64,
}

pub fn abbrev_position(pos: &str) -> &str {
    match pos {
        "Goalkeeper" => "GK",
        "Right Back" => "RB",
        "Left Back" => "LB",
        "Center Back" => "CB",
        "Right Center Back" => "RCB",
        "Left Center Back" => "LCB",
        "Right Wing Back" => "RWB",
        "Left Wing Back" => "LWB",
        "Defensive Midfield" => "DM",
        "Right Defensive Midfield" => "RDM",
        "Left Defensive Midfield" => "LDM",
        "Center Midfield" => "CM",
        "Right Center Midfield" => "RCM",
        "Left Center Midfield" => "LCM",
        "Attacking Midfield" => "AM",
        "Right Midfield" => "RM",
        "Left Midfield" => "LM",
        "Right Wing" => "RW",
        "Left Wing" => "LW",
        "Center Forward" => "CF",
        "Right Center Forward" => "RCF",
        "Left Center Forward" => "LCF",
        "Secondary Striker" => "SS",
        "Center Defensive Midfield" => "CDM",
        "Center Attacking Midfield" => "CAM",
        "Left Attacking Midfield" => "LAM",
        "Right Attacking Midfield" => "RAM",
        other => other,
    }
}

fn find_lineup<'a>(state: &'a MatchState, team: &str) -> Option<&'a TeamLineup> {
    if state.home_lineup.team_name.eq_ignore_ascii_case(team) {
        Some(&state.home_lineup)
    } else if state.away_lineup.team_name.eq_ignore_ascii_case(team) {
        Some(&state.away_lineup)
    } else {
        None
    }
}

/// Build a season-wide APM baseline for each player from historical matches.
/// For each match, counts active events per player and divides by 90 to get APM,
/// then averages across all matches the player appeared in.
pub fn build_season_baseline(
    data_dir: &Path,
    team: &str,
    match_ids: &[String],
) -> HashMap<String, f64> {
    // player_name -> (total_active_events, matches_appeared_in)
    let mut totals: HashMap<String, (u32, u32)> = HashMap::new();

    for match_id in match_ids {
        let Ok(events) = crate::ingest::load_events_only(data_dir, match_id) else {
            continue;
        };
        let mut per_match: HashMap<String, u32> = HashMap::new();
        for event in &events {
            if !event.team_name.eq_ignore_ascii_case(team) {
                continue;
            }
            if !ACTIVE_TYPES.contains(&event.event_type.as_str()) {
                continue;
            }
            let Some(player) = &event.player_name else {
                continue;
            };
            *per_match.entry(player.clone()).or_insert(0) += 1;
        }
        for (player, count) in per_match {
            let entry = totals.entry(player).or_insert((0, 0));
            entry.0 += count;
            entry.1 += 1;
        }
    }

    totals
        .into_iter()
        .map(|(player, (total, count))| (player, total as f64 / (count as f64 * 90.0)))
        .collect()
}

/// Compute per-player decay scores.
/// If `season_baseline` is provided, each player's baseline APM is taken from
/// their historical season average rather than the current match's first half.
/// Falls back to within-match baseline for players absent from the history.
pub fn compute_decay(
    state: &MatchState,
    team: &str,
    current_minute: u32,
    season_baseline: Option<&HashMap<String, f64>>,
) -> Vec<DecayScore> {
    let baseline_end = (current_minute as f64 / 2.0).min(45.0);
    let recent_start = (current_minute as f64 - 15.0).max(0.0);
    let baseline_duration = baseline_end;
    let recent_duration = (current_minute as f64 - recent_start).max(1.0);

    let lineup = match find_lineup(state, team) {
        Some(l) => l,
        None => return vec![],
    };

    let mut baseline_counts: HashMap<&str, u32> = HashMap::new();
    let mut recent_counts: HashMap<&str, u32> = HashMap::new();
    let mut match_counts: HashMap<&str, u32> = HashMap::new();

    for event in &state.events {
        if !event.team_name.eq_ignore_ascii_case(team) {
            continue;
        }
        if !ACTIVE_TYPES.contains(&event.event_type.as_str()) {
            continue;
        }
        let player = match event.player_name.as_deref() {
            Some(p) => p,
            None => continue,
        };
        let min = event.minute as f64;
        if min <= current_minute as f64 {
            *match_counts.entry(player).or_insert(0) += 1;
        }
        if min <= baseline_end {
            *baseline_counts.entry(player).or_insert(0) += 1;
        }
        if min >= recent_start && min <= current_minute as f64 {
            *recent_counts.entry(player).or_insert(0) += 1;
        }
    }

    let mut scores: Vec<DecayScore> = lineup
        .players
        .iter()
        .map(|p| {
            let name = p.player_name.as_str();
            let baseline_apm = match season_baseline.and_then(|sb| sb.get(name).copied()) {
                Some(apm) => apm,
                None => {
                    if baseline_duration > 0.0 {
                        baseline_counts.get(name).copied().unwrap_or(0) as f64 / baseline_duration
                    } else {
                        0.0
                    }
                }
            };
            let recent_apm = recent_counts.get(name).copied().unwrap_or(0) as f64 / recent_duration;
            let has_played = match_counts.get(name).copied().unwrap_or(0) > 0;
            let is_gk = p.position == "Goalkeeper";
            let score = if baseline_apm == 0.0 || !has_played || is_gk {
                0.0
            } else {
                1.0 - (recent_apm / baseline_apm).min(1.0)
            };

            DecayScore {
                player_name: p.player_name.clone(),
                position: abbrev_position(&p.position).to_string(),
                position_full: p.position.clone(),
                baseline_apm,
                recent_apm,
                score,
            }
        })
        .collect();

    scores.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    scores
}
