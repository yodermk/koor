use super::Frontend;
use crate::{game::Phase, session::Session};

pub struct CliFrontend {
    pub rounds: u64,
    pub trace: bool,
    pub quiet: bool,
}

impl Frontend for CliFrontend {
    fn run(self, mut session: Session) -> Result<(), String> {
        let mut completed = 0;
        let mut steps = 0;
        let mut event_count = 0;
        while completed < self.rounds {
            steps += 1;
            if steps > self.rounds.saturating_mul(1000) {
                return Err("Simulation exceeded its action limit.".into());
            }
            if matches!(session.game.phase(), Phase::Complete | Phase::AllPassed) {
                if let Some(result) = session.view(0).result {
                    completed += 1;
                    if !self.quiet {
                        println!(
                            "Round {}: seat {} bid {}, partner seat {}, captured {}, {}. Scores {:?}",
                            session.round,
                            result.leader,
                            result.bid,
                            result.partner,
                            result.team_points,
                            if result.made_bid { "made" } else { "missed" },
                            result.scores
                        );
                    }
                    if let Some(warning) = &session.warning {
                        eprintln!("{warning}");
                    }
                }
                if completed < self.rounds {
                    session.next_round().map_err(|error| error.to_string())?;
                    event_count = 0;
                }
            } else {
                session.step_bot().map_err(|error| error.to_string())?;
            }
            if self.trace {
                let view = session.view(0);
                for event in &view.events[event_count..] {
                    println!("  {event}");
                }
                event_count = view.events.len();
            }
        }
        println!(
            "Completed {completed} rounds. Totals by seat [0, 1, 2, 3]: {:?}",
            session.totals
        );
        Ok(())
    }
}
