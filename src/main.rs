mod ingest;

use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "subwatch", about = "StatsBomb match analysis tool")]
struct Args {
    /// Match ID to load
    #[arg(long)]
    match_id: String,

    /// Team name filter (currently informational)
    #[arg(long)]
    team: Option<String>,

    /// Path to StatsBomb open-data `data/` directory
    #[arg(long, default_value = "data/open-data/data")]
    data_dir: PathBuf,
}

fn main() -> Result<()> {
    let args = Args::parse();

    let state = ingest::load_match(&args.data_dir, &args.match_id)?;

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

    if let Some(team) = &args.team {
        let team_events: Vec<_> = state
            .events
            .iter()
            .filter(|e| e.team_name.eq_ignore_ascii_case(team))
            .collect();
        println!("\nEvents for '{}': {}", team, team_events.len());
    }

    Ok(())
}
