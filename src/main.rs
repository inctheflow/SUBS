mod dashboard;
mod decay;
mod ingest;
mod matcher;

use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "subs", about = "StatsBomb match analysis tool")]
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

    /// Print debug info for bench candidate scoring
    #[arg(long, default_value_t = false)]
    verbose: bool,

    /// Launch full-screen terminal dashboard
    #[arg(long, default_value_t = false)]
    dashboard: bool,

    /// Use season-wide APM baseline built from all historical matches for the team
    #[arg(long, default_value_t = false)]
    season_baseline: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();

    if args.dashboard {
        let team = args.team.unwrap_or_else(|| {
            eprintln!("--dashboard requires --team");
            std::process::exit(1);
        });
        let app = dashboard::App::new(
            args.data_dir,
            args.match_id,
            team,
            args.minute,
            args.verbose,
            args.season_baseline,
        )?;
        return dashboard::run(app);
    }

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
        let season_bl = if args.season_baseline {
            let match_ids = ingest::find_team_match_ids(&args.data_dir, team, &args.match_id);
            if match_ids.is_empty() {
                eprintln!("No historical matches found for '{}' — falling back to within-match baseline.", team);
                None
            } else {
                println!("Building season baseline from {} matches…", match_ids.len());
                Some(decay::build_season_baseline(&args.data_dir, team, &match_ids))
            }
        } else {
            None
        };

        let scores = decay::compute_decay(&state, team, args.minute, season_bl.as_ref());

        if scores.is_empty() {
            println!("\nNo lineup found for team '{}'.", team);
            return Ok(());
        }

        // Build subbed-off set for the decay table marker
        let subbed_off: std::collections::HashSet<&str> = state
            .events
            .iter()
            .filter(|e| {
                e.event_type == "Substitution"
                    && e.team_name.eq_ignore_ascii_case(team)
                    && e.minute <= args.minute
            })
            .filter_map(|e| e.player_name.as_deref())
            .collect();

        println!(
            "\nDecay scores for '{}' at minute {} (highest decay first):",
            team, args.minute
        );
        println!(
            "{:<22} | {:<5} | {:>12} | {:>10} | {:>5}",
            "PLAYER", "POS", "BASELINE APM", "RECENT APM", "DECAY"
        );
        println!("{}", "-".repeat(65));
        let mut any_subbed = false;
        for s in &scores {
            let is_subbed = subbed_off.contains(s.player_name.as_str());
            if is_subbed {
                any_subbed = true;
            }
            // Reserve 2 chars for " *" marker on subbed players
            let (char_limit, ellipsis_at) = if is_subbed { (20, 19) } else { (22, 21) };
            let base = if s.player_name.chars().count() > char_limit {
                let cut: String = s.player_name.chars().take(ellipsis_at).collect();
                format!("{}…", cut)
            } else {
                s.player_name.clone()
            };
            let display = if is_subbed {
                format!("{} *", base)
            } else {
                base
            };
            println!(
                "{:<22} | {:<5} | {:>12.2} | {:>10.2} | {:>5.2}",
                display, s.position, s.baseline_apm, s.recent_apm, s.score
            );
        }
        if any_subbed {
            println!("  * = already substituted off");
        }

        // Phase 3: bench matcher + recommendation card
        match matcher::recommend(&state, team, &scores, args.minute, args.verbose) {
        None => println!(
            "\nNo substitution recommended at minute {} — all players performing at baseline.",
            args.minute
        ),
        Some(rec) => {
            let sep = "═".repeat(55);
            println!("\n{}", sep);
            println!("  SUBS RECOMMENDATION — minute {}", args.minute);
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
        }}
    }

    Ok(())
}
