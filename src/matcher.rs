use crate::decay::{abbrev_position, DecayScore};
use crate::ingest::MatchState;
use std::collections::{HashMap, HashSet};

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

#[allow(dead_code)]
pub struct BenchCandidate {
    pub player_name: String,
    pub position: String,
    pub total_actions: u32,
    pub press_count: u32,
    pub duel_won_count: u32,
    pub fit_score: f64,
}

pub struct Recommendation {
    pub tactical_problem: String,
    pub take_off: Vec<(String, String, f64)>, // (name, pos, decay_score)
    pub bring_on: Vec<BenchCandidate>,
}

fn problem_for_pos(pos: &str) -> &'static str {
    if pos.contains("CB") || pos == "GK" {
        "Defensive stability weakening"
    } else if pos == "LB" || pos == "RB" || pos.contains("WB") {
        "Flank coverage dropping"
    } else if pos.contains("CM") || pos.contains("DM") {
        "Midfield press collapsing"
    } else if pos == "LW" || pos == "RW" || pos == "AM" || pos == "LM" || pos == "RM" {
        "Attacking width lost"
    } else if pos.contains("CF") || pos == "SS" {
        "Striker pressing fading"
    } else {
        "General fatigue"
    }
}

fn pos_group(pos: &str) -> &'static str {
    if pos.contains("CB") || pos == "GK" {
        "def_central"
    } else if pos == "LB" || pos == "RB" || pos.contains("WB") {
        "def_flank"
    } else if pos.contains("CM") || pos.contains("DM") {
        "mid"
    } else if pos == "LW" || pos == "RW" || pos == "AM" || pos == "LM" || pos == "RM" {
        "att_wide"
    } else if pos.contains("CF") || pos == "SS" {
        "att_central"
    } else {
        "unknown"
    }
}

fn problem_group(problem: &str) -> &'static str {
    match problem {
        "Defensive stability weakening" => "def_central",
        "Flank coverage dropping" => "def_flank",
        "Midfield press collapsing" => "mid",
        "Attacking width lost" => "att_wide",
        "Striker pressing fading" => "att_central",
        _ => "unknown",
    }
}

pub fn recommend(
    state: &MatchState,
    team: &str,
    decay_scores: &[DecayScore],
    current_minute: u32,
) -> Option<Recommendation> {
    let lineup = if state.home_lineup.team_name.eq_ignore_ascii_case(team) {
        &state.home_lineup
    } else if state.away_lineup.team_name.eq_ignore_ascii_case(team) {
        &state.away_lineup
    } else {
        return None;
    };

    // Players with any event before current_minute (proxy for "has been on the pitch")
    let players_with_events: HashSet<&str> = state
        .events
        .iter()
        .filter(|e| e.team_name.eq_ignore_ascii_case(team) && e.minute < current_minute)
        .filter_map(|e| e.player_name.as_deref())
        .collect();

    // Players subbed off at or before current_minute
    let subbed_off: HashSet<&str> = state
        .events
        .iter()
        .filter(|e| {
            e.event_type == "Substitution"
                && e.team_name.eq_ignore_ascii_case(team)
                && e.minute <= current_minute
        })
        .filter_map(|e| e.player_name.as_deref())
        .collect();

    // Players who came on as a sub at or before current_minute
    let came_on: HashSet<&str> = state
        .events
        .iter()
        .filter(|e| {
            e.event_type == "Substitution"
                && e.team_name.eq_ignore_ascii_case(team)
                && e.minute <= current_minute
        })
        .filter_map(|e| e.substitution_player_on.as_deref())
        .collect();

    // Field = has events OR came on as sub, AND not subbed off
    let field_names: HashSet<&str> = lineup
        .players
        .iter()
        .filter(|p| {
            let name = p.player_name.as_str();
            (players_with_events.contains(name) || came_on.contains(name))
                && !subbed_off.contains(name)
        })
        .map(|p| p.player_name.as_str())
        .collect();

    // Bench = in lineup, not currently on the pitch, and not already used (subbed off)
    let bench_players: Vec<_> = lineup
        .players
        .iter()
        .filter(|p| {
            let name = p.player_name.as_str();
            !field_names.contains(name) && !subbed_off.contains(name)
        })
        .collect();

    // Top fatigued field players (score > 0.40, already sorted by decay_scores)
    let fatigued: Vec<&DecayScore> = decay_scores
        .iter()
        .filter(|s| field_names.contains(s.player_name.as_str()) && s.score > 0.40)
        .take(3)
        .collect();

    if fatigued.is_empty() {
        return None;
    }

    // Dominant tactical problem across top fatigued players
    let mut problem_counts: HashMap<&str, usize> = HashMap::new();
    for s in &fatigued {
        *problem_counts.entry(problem_for_pos(&s.position)).or_insert(0) += 1;
    }
    let tactical_problem = problem_counts
        .into_iter()
        .max_by_key(|(_, v)| *v)
        .map(|(k, _)| k)
        .unwrap_or("General fatigue");

    let take_off: Vec<_> = fatigued
        .iter()
        .take(2)
        .map(|s| (s.player_name.clone(), s.position.clone(), s.score))
        .collect();

    // Build bench candidates with fit scores
    let target_group = problem_group(tactical_problem);

    let mut bring_on: Vec<BenchCandidate> = bench_players
        .iter()
        .map(|p| {
            let abbrev = abbrev_position(&p.position).to_string();

            let total_actions = state
                .events
                .iter()
                .filter(|e| {
                    e.team_name.eq_ignore_ascii_case(team)
                        && e.minute <= current_minute
                        && e.player_name.as_deref() == Some(p.player_name.as_str())
                        && ACTIVE_TYPES.contains(&e.event_type.as_str())
                })
                .count() as u32;

            let press_count = state
                .events
                .iter()
                .filter(|e| {
                    e.team_name.eq_ignore_ascii_case(team)
                        && e.minute <= current_minute
                        && e.player_name.as_deref() == Some(p.player_name.as_str())
                        && e.event_type == "Pressure"
                })
                .count() as u32;

            let duel_won_count = state
                .events
                .iter()
                .filter(|e| {
                    e.team_name.eq_ignore_ascii_case(team)
                        && e.minute <= current_minute
                        && e.player_name.as_deref() == Some(p.player_name.as_str())
                        && e.event_type == "Duel"
                        && e.duel_outcome
                            .as_deref()
                            .map(|o| o.contains("Won"))
                            .unwrap_or(false)
                })
                .count() as u32;

            let position_match = pos_group(&abbrev) == target_group;
            let fit_score =
                if position_match { 2.0 } else { 0.0 } + if total_actions > 0 { 1.0 } else { 0.0 };

            BenchCandidate {
                player_name: p.player_name.clone(),
                position: abbrev,
                total_actions,
                press_count,
                duel_won_count,
                fit_score,
            }
        })
        .collect();

    bring_on.sort_by(|a, b| {
        b.fit_score
            .partial_cmp(&a.fit_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    Some(Recommendation {
        tactical_problem: tactical_problem.to_string(),
        take_off,
        bring_on,
    })
}
