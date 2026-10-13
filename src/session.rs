use crate::{
    ai::{self, Style},
    game::{Action, Game, GameError, Phase, View},
    learning::Learning,
};
use rand::{SeedableRng, rngs::StdRng};
use std::{io, path::PathBuf};

pub struct Session {
    pub game: Game,
    pub totals: [i64; 4],
    pub round: u64,
    pub learning: Learning,
    pub warning: Option<String>,
    rng: StdRng,
    learning_path: Option<PathBuf>,
    human: Option<usize>,
    choices: [(usize, usize); 4],
    bid_ceilings: [u16; 4],
    recorded: bool,
}

impl Session {
    pub fn new(
        seed: u64,
        human: Option<usize>,
        learning_path: Option<PathBuf>,
    ) -> io::Result<Self> {
        let learning = match &learning_path {
            Some(path) => Learning::load(path)?,
            None => Learning::default(),
        };
        let mut rng = StdRng::seed_from_u64(seed);
        let game = Game::new(&mut rng);
        let mut session = Self {
            game,
            totals: [0; 4],
            round: 1,
            learning,
            warning: None,
            rng,
            learning_path,
            human,
            choices: [(0, 1); 4],
            bid_ceilings: [0; 4],
            recorded: false,
        };
        session.select_styles();
        Ok(session)
    }

    pub fn view(&self, player: usize) -> View {
        self.game.view(player)
    }

    pub fn act(&mut self, player: usize, action: Action) -> Result<(), GameError> {
        self.game.apply(player, action)?;
        self.record_result();
        Ok(())
    }

    pub fn step_bot(&mut self) -> Result<bool, GameError> {
        let Some(player) = self.game.turn() else {
            return Ok(false);
        };
        if self.human == Some(player) {
            return Ok(false);
        }
        let action = ai::choose_action_with_bid_ceiling(
            &self.view(player),
            Style::from_index(self.choices[player].1),
            self.bid_ceilings[player],
        );
        self.act(player, action)?;
        Ok(true)
    }

    pub fn next_round(&mut self) -> Result<(), GameError> {
        if !matches!(self.game.phase(), Phase::Complete | Phase::AllPassed) {
            return Err(GameError("Finish this round first."));
        }
        if self.game.phase() == Phase::Complete {
            self.round += 1;
        }
        self.game = Game::new(&mut self.rng);
        self.recorded = false;
        self.select_styles();
        Ok(())
    }

    fn select_styles(&mut self) {
        for player in 0..4 {
            let hand = self.view(player).hand;
            let context = usize::from(ai::estimate_bid(&hand) >= 120);
            let style = if self.learning_path.is_some() {
                self.learning.select(context, &mut self.rng)
            } else {
                1
            };
            self.choices[player] = (context, style);
            self.bid_ceilings[player] =
                ai::bid_ceiling(&hand, Style::from_index(style), &mut self.rng);
        }
    }

    fn record_result(&mut self) {
        if self.recorded {
            return;
        }
        let Some(result) = self.view(0).result else {
            return;
        };
        self.recorded = true;
        for player in 0..4 {
            self.totals[player] += i64::from(result.scores[player]);
            if self.learning_path.is_some() && self.human != Some(player) {
                let (context, style) = self.choices[player];
                self.learning.observe(context, style, result.scores[player]);
            }
        }
        if let Some(path) = &self.learning_path {
            self.learning.rounds += 1;
            self.warning = self
                .learning
                .save(path)
                .err()
                .map(|error| format!("Could not save learning state: {error}"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bidding_ceilings_stay_fixed_until_the_next_deal() {
        let mut session = Session::new(42, None, None).unwrap();
        let first_ceilings = session.bid_ceilings;
        while session.game.phase() == Phase::Bidding {
            assert!(session.step_bot().unwrap());
            assert_eq!(session.bid_ceilings, first_ceilings);
        }
        while session.step_bot().unwrap() {}
        session.next_round().unwrap();
        assert_ne!(session.bid_ceilings, first_ceilings);

        // An all-pass redeal also generates fresh ceilings.
        let second_ceilings = session.bid_ceilings;
        while let Some(player) = session.game.turn() {
            session.act(player, Action::Pass).unwrap();
        }
        assert_eq!(session.game.phase(), Phase::AllPassed);
        session.next_round().unwrap();
        assert_ne!(session.bid_ceilings, second_ceilings);
    }
}
