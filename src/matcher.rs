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

const DECAY_THRESHOLD: f64 = 0.25;

#[allow(dead_code)]
pub struct BenchCandidate {
    pub player_name: String,
    pub position: String,      // abbreviated, for display
    pub position_full: String, // full StatsBomb string, for group matching
    pub jersey_number: u8,
    pub total_actions: u32,
    pub press_count: u32,
    pub duel_won_count: u32,
    pub fit_score: f64,
}

pub struct Recommendation {
    pub tactical_problem: String,
    pub take_off: Vec<(String, String, f64)>, // (name, pos_abbrev, decay_score)
    pub bring_on: Vec<BenchCandidate>,
}

// Map full StatsBomb position string → tactical problem label
fn problem_for_pos_full(pos: &str) -> &'static str {
    match pos {
        "Goalkeeper" => "Goalkeeper fatigue",
        "Center Back" | "Left Center Back" | "Right Center Back" => {
            "Defensive stability weakening"
        }
        "Left Back" | "Right Back" | "Left Wing Back" | "Right Wing Back" => {
            "Flank coverage dropping"
        }
        "Center Midfield"
        | "Left Center Midfield"
        | "Right Center Midfield"
        | "Center Defensive Midfield"
        | "Left Defensive Midfield"
        | "Right Defensive Midfield"
        | "Defensive Midfield" => "Midfield press collapsing",
        "Left Wing"
        | "Right Wing"
        | "Left Midfield"
        | "Right Midfield"
        | "Center Attacking Midfield"
        | "Left Attacking Midfield"
        | "Right Attacking Midfield"
        | "Attacking Midfield" => "Attacking width lost",
        "Center Forward"
        | "Left Center Forward"
        | "Right Center Forward"
        | "Secondary Striker" => "Striker pressing fading",
        _ => "General fatigue",
    }
}

// Map full StatsBomb position string → position group for bench matching
fn pos_group_full(pos: &str) -> &'static str {
    match pos {
        "Goalkeeper" => "gk",
        "Center Back" | "Left Center Back" | "Right Center Back" => "def_central",
        "Left Back" | "Right Back" | "Left Wing Back" | "Right Wing Back" => "def_flank",
        "Center Midfield"
        | "Left Center Midfield"
        | "Right Center Midfield"
        | "Center Defensive Midfield"
        | "Left Defensive Midfield"
        | "Right Defensive Midfield"
        | "Defensive Midfield" => "mid",
        "Left Wing"
        | "Right Wing"
        | "Left Midfield"
        | "Right Midfield"
        | "Center Attacking Midfield"
        | "Left Attacking Midfield"
        | "Right Attacking Midfield"
        | "Attacking Midfield" => "att_wide",
        "Center Forward"
        | "Left Center Forward"
        | "Right Center Forward"
        | "Secondary Striker" => "att_central",
        _ => "unknown",
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
    verbose: bool,
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

    // Top fatigued field players above threshold, using full position for problem detection
    let fatigued: Vec<&DecayScore> = decay_scores
        .iter()
        .filter(|s| field_names.contains(s.player_name.as_str()) && s.score > DECAY_THRESHOLD)
        .take(3)
        .collect();

    if fatigued.is_empty() {
        return None;
    }

    // Dominant tactical problem across top fatigued field players (full string matching)
    let mut problem_counts: HashMap<&str, usize> = HashMap::new();
    for s in &fatigued {
        *problem_counts
            .entry(problem_for_pos_full(&s.position_full))
            .or_insert(0) += 1;
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

    let target_group = problem_group(tactical_problem);

    // Build bench candidates — match against full StatsBomb position strings
    let mut bring_on: Vec<BenchCandidate> = bench_players
        .iter()
        .map(|p| {
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

            let position_match = pos_group_full(&p.position) == target_group;
            // +2.0 position match, +0.5 warmup bonus for any prior events
            let fit_score = if position_match { 2.0 } else { 0.0 }
                + if total_actions > 0 { 0.5 } else { 0.0 };

            BenchCandidate {
                player_name: p.player_name.clone(),
                position: abbrev_position(&p.position).to_string(),
                position_full: p.position.clone(),
                jersey_number: p.jersey_number,
                total_actions,
                press_count,
                duel_won_count,
                fit_score,
            }
        })
        .collect();

    if verbose {
        println!("\n  [debug] bench candidates (pre-sort):");
        for c in &bring_on {
            println!(
                "    {:<30} | pos: {:<5} | group: {:<12} | target: {:<12} | fit: {:.1}",
                c.player_name,
                c.position,
                pos_group_full(&c.position_full),
                target_group,
                c.fit_score
            );
        }
    }

    let all_zero = bring_on.iter().all(|c| c.fit_score == 0.0);

    if all_zero {
        // No position match — rank by freshness (fewer actions = more rested), then jersey number
        bring_on.sort_by(|a, b| {
            a.total_actions
                .cmp(&b.total_actions)
                .then_with(|| a.jersey_number.cmp(&b.jersey_number))
        });
    } else {
        // fit_score desc, jersey number asc as tiebreak
        bring_on.sort_by(|a, b| {
            b.fit_score
                .partial_cmp(&a.fit_score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.jersey_number.cmp(&b.jersey_number))
        });
    }

    Some(Recommendation {
        tactical_problem: tactical_problem.to_string(),
        take_off,
        bring_on,
    })
}
