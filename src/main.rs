mod decay;
mod ingest;
mod matcher;

use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "subwatch", about = "StatsBomb match analysis tool")]
struct Args {
    /// Match ID to load
    #[arg(long)]
    match_id: String,

    /// Team name filter
    #[arg(long)]
    team: Option<String>,

    /// Path to StatsBomb open-data `data/` directory
    #[arg(long, default_value = "data/open-data/data")]
    data_dir: PathBuf,

    /// Simulate current match minute for decay scoring (default 90)
    #[arg(long, default_value_t = 90)]
    minute: u32,
}

fn main() -> Result<()> {
    let args = Args::parse();

    let state = ingest::load_match(&args.data_dir, &args.match_id)?;

    // Phase 1 output
    println!("Total events loaded: {}", state.events.len());
    println!(
        "Home team: {} ({} players)",
        state.home_lineup.team_name,
        state.home_lineup.players.len()
    );
    println!(
        "Away team: {} ({} players)",
        state.away_lineup.team_name,
        state.away_lineup.players.len()
    );

    println!("\nFirst 5 events:");
    for event in state.events.iter().take(5) {
        let player = event.player_name.as_deref().unwrap_or("-");
        println!(
            "  [{:>3}'] {:.<30} {}",
            event.minute, event.event_type, player
        );
    }

    if let Some(ref team) = args.team {
        let team_events: Vec<_> = state
            .events
            .iter()
            .filter(|e| e.team_name.eq_ignore_ascii_case(team))
            .collect();
        println!("\nEvents for '{}': {}", team, team_events.len());
    }

    // Phase 2: decay engine
    if let Some(ref team) = args.team {
        let scores = decay::compute_decay(&state, team, args.minute);

        if scores.is_empty() {
            println!("\nNo lineup found for team '{}'.", team);
            return Ok(());
        }

        println!(
            "\nDecay scores for '{}' at minute {} (highest decay first):",
            team, args.minute
        );
        println!(
            "{:<22} | {:<5} | {:>12} | {:>10} | {:>5}",
            "PLAYER", "POS", "BASELINE APM", "RECENT APM", "DECAY"
        );
        println!("{}", "-".repeat(65));
        for s in &scores {
            let name = if s.player_name.chars().count() > 22 {
                let cut: String = s.player_name.chars().take(21).collect();
                format!("{}…", cut)
            } else {
                s.player_name.clone()
            };
            println!(
                "{:<22} | {:<5} | {:>12.2} | {:>10.2} | {:>5.2}",
                name, s.position, s.baseline_apm, s.recent_apm, s.score
            );
        }

        // Phase 3: bench matcher + recommendation card
        if let Some(rec) = matcher::recommend(&state, team, &scores, args.minute) {
            let sep = "═".repeat(55);
            println!("\n{}", sep);
            println!("  SUBWATCH RECOMMENDATION — minute {}", args.minute);
            println!("{}", sep);
            println!("  TACTICAL PROBLEM: {}", rec.tactical_problem);

            println!("\n  TAKE OFF:");
            for (i, (name, pos, decay)) in rec.take_off.iter().enumerate() {
                println!("    {}. {} ({}) — decay {:.2}", i + 1, name, pos, decay);
            }

            println!("\n  BRING ON:");
            let candidates: Vec<_> = rec.bring_on.iter().take(3).collect();
            for (i, c) in candidates.iter().enumerate() {
                let suffix = if i == 0 {
                    "  [BEST MATCH]"
                } else if i == 2 {
                    "  (if available)"
                } else {
                    ""
                };
                println!(
                    "    {}. {} ({}) — fit score {:.1}{}",
                    i + 1,
                    c.player_name,
                    c.position,
                    c.fit_score,
                    suffix
                );
            }
            println!("{}", sep);
        }
    }

    Ok(())
}
