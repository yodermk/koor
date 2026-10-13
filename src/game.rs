use crate::cards::{Card, Suit, sort_hand};
use rand::{Rng, seq::SliceRandom};
use std::{collections::HashSet, fmt};

pub type Player = usize;
pub const NAMES: [&str; 4] = ["You", "Ada", "Linus", "Grace"];

fn verb(player: Player, you: &'static str, other: &'static str) -> &'static str {
    if player == 0 { you } else { other }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Bidding,
    Discard,
    Declare,
    Playing,
    Complete,
    AllPassed,
}

#[derive(Clone, Debug)]
pub enum Action {
    Bid(u16),
    Pass,
    Discard(Vec<Card>),
    Declare { trump: Suit, partner: Card },
    Play(Card),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GameError(pub &'static str);

impl fmt::Display for GameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}
impl std::error::Error for GameError {}

#[derive(Clone, Debug)]
pub struct RoundResult {
    pub leader: Player,
    pub partner: Player,
    pub bid: u16,
    pub captured: [u16; 4],
    pub team_points: u16,
    pub made_bid: bool,
    pub scores: [i32; 4],
}

/// The only input to a player or UI: private cards belong to this seat only.
#[derive(Clone, Debug)]
pub struct View {
    pub player: Player,
    pub phase: Phase,
    pub turn: Option<Player>,
    pub hand: Vec<Card>,
    pub hand_sizes: [usize; 4],
    pub legal_cards: Vec<Card>,
    pub partner_choices: Vec<Card>,
    pub high_bid: u16,
    pub leader: Option<Player>,
    pub trump: Option<Suit>,
    pub partner_card: Option<Card>,
    pub revealed_partner: Option<Player>,
    pub known_partner: Option<Player>,
    pub kitty_has_points: Option<bool>,
    pub trick: Vec<(Player, Card)>,
    pub last_trick: Vec<(Player, Card)>,
    pub last_winner: Option<Player>,
    pub played: Vec<Card>,
    pub captured: [u16; 4],
    pub tricks_finished: usize,
    pub events: Vec<String>,
    pub result: Option<RoundResult>,
}

pub struct Game {
    hands: [Vec<Card>; 4],
    kitty: Vec<Card>,
    phase: Phase,
    turn: Player,
    passed: [bool; 4],
    high_bid: u16,
    leader: Option<Player>,
    trump: Option<Suit>,
    partner_card: Option<Card>,
    partner: Option<Player>,
    revealed_partner: Option<Player>,
    trick: Vec<(Player, Card)>,
    last_trick: Vec<(Player, Card)>,
    last_winner: Option<Player>,
    played: Vec<Card>,
    captured: [u16; 4],
    tricks_finished: usize,
    events: Vec<String>,
    result: Option<RoundResult>,
}

pub fn trick_winner(trick: &[(Player, Card)], trump: Suit) -> Option<Player> {
    let lead = trick.first()?.1.suit(trump);
    trick
        .iter()
        .max_by_key(|(_, card)| {
            let suit = card.suit(trump);
            let priority = if suit == trump {
                2
            } else if suit == lead {
                1
            } else {
                0
            };
            (priority, card.strength())
        })
        .map(|(player, _)| *player)
}

impl Game {
    pub fn new(rng: &mut impl Rng) -> Self {
        let mut deck = Card::deck();
        deck.shuffle(rng);
        let hands = std::array::from_fn(|player| {
            let mut hand = deck[player * 10..player * 10 + 10].to_vec();
            sort_hand(&mut hand, None);
            hand
        });
        let turn = rng.random_range(0..4);
        Self {
            hands,
            kitty: deck[40..].to_vec(),
            phase: Phase::Bidding,
            turn,
            passed: [false; 4],
            high_bid: 0,
            leader: None,
            trump: None,
            partner_card: None,
            partner: None,
            revealed_partner: None,
            trick: vec![],
            last_trick: vec![],
            last_winner: None,
            played: vec![],
            captured: [0; 4],
            tricks_finished: 0,
            events: vec![format!(
                "Cards dealt. {} {} the bidding.",
                NAMES[turn],
                verb(turn, "open", "opens")
            )],
            result: None,
        }
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    pub fn turn(&self) -> Option<Player> {
        (!matches!(self.phase, Phase::Complete | Phase::AllPassed)).then_some(self.turn)
    }

    pub fn view(&self, player: Player) -> View {
        assert!(player < 4, "player must be a seat from 0 to 3");
        let legal_cards = if self.phase == Phase::Playing && self.turn == player {
            let trump = self.trump.unwrap();
            let lead = self.trick.first().map(|(_, card)| card.suit(trump));
            let follows = lead.is_some_and(|suit| {
                self.hands[player]
                    .iter()
                    .any(|card| card.suit(trump) == suit)
            });
            self.hands[player]
                .iter()
                .copied()
                .filter(|card| !follows || Some(card.suit(trump)) == lead)
                .collect()
        } else {
            vec![]
        };
        let partner_choices = if self.phase == Phase::Declare && self.turn == player {
            Card::deck()
                .into_iter()
                .filter(|card| !self.kitty.contains(card))
                .collect()
        } else {
            vec![]
        };
        let known_partner = self.revealed_partner.or_else(|| {
            self.partner_card
                .and_then(|card| self.hands[player].contains(&card).then_some(player))
        });
        View {
            player,
            phase: self.phase,
            turn: self.turn(),
            hand: self.hands[player].clone(),
            hand_sizes: std::array::from_fn(|p| self.hands[p].len()),
            legal_cards,
            partner_choices,
            high_bid: self.high_bid,
            leader: self.leader,
            trump: self.trump,
            partner_card: self.partner_card,
            revealed_partner: self.revealed_partner,
            known_partner,
            kitty_has_points: self
                .trump
                .map(|_| self.kitty.iter().any(|card| card.points() > 0)),
            trick: self.trick.clone(),
            last_trick: self.last_trick.clone(),
            last_winner: self.last_winner,
            played: self.played.clone(),
            captured: self.captured,
            tricks_finished: self.tricks_finished,
            events: self.events.clone(),
            result: self.result.clone(),
        }
    }

    /// Invalid actions leave the entire round unchanged.
    pub fn apply(&mut self, player: Player, action: Action) -> Result<(), GameError> {
        if self.turn() != Some(player) {
            return Err(GameError("It is not your turn."));
        }
        match (self.phase, action) {
            (Phase::Bidding, Action::Bid(bid)) => {
                if bid > 180 || bid < self.high_bid + 5 || !bid.is_multiple_of(5) {
                    return Err(GameError(
                        "Raise by at least 5, in multiples of 5, up to 180.",
                    ));
                }
                self.high_bid = bid;
                self.leader = Some(player);
                self.events.push(format!(
                    "{} {} {bid}.",
                    NAMES[player],
                    verb(player, "bid", "bids")
                ));
                if bid == 180 {
                    self.finish_bidding();
                } else {
                    self.next_bidder();
                }
            }
            (Phase::Bidding, Action::Pass) => {
                self.passed[player] = true;
                self.events.push(format!(
                    "{} {}.",
                    NAMES[player],
                    verb(player, "pass", "passes")
                ));
                self.next_bidder();
            }
            (Phase::Discard, Action::Discard(cards)) => {
                if cards.len() != 5
                    || cards.iter().collect::<HashSet<_>>().len() != 5
                    || cards.iter().any(|card| !self.hands[player].contains(card))
                {
                    return Err(GameError(
                        "Select exactly five different cards from your hand.",
                    ));
                }
                self.hands[player].retain(|card| !cards.contains(card));
                self.kitty = cards;
                self.phase = Phase::Declare;
                self.events.push(format!(
                    "{} {} five cards face down.",
                    NAMES[player],
                    verb(player, "discard", "discards")
                ));
            }
            (Phase::Declare, Action::Declare { trump, partner }) => {
                if !Card::deck().contains(&partner) || self.kitty.contains(&partner) {
                    return Err(GameError(
                        "The partner card must be in a player's hand, not the kitty.",
                    ));
                }
                self.partner = self.hands.iter().position(|hand| hand.contains(&partner));
                self.trump = Some(trump);
                for hand in &mut self.hands {
                    sort_hand(hand, self.trump);
                }
                self.partner_card = Some(partner);
                self.phase = Phase::Playing;
                let points = if self.kitty.iter().any(|card| card.points() > 0) {
                    "has points"
                } else {
                    "has no points"
                };
                self.events.push(format!(
                    "{} {} {trump} trump and {partner} as partner. The kitty {points}.",
                    NAMES[player],
                    verb(player, "call", "calls")
                ));
            }
            (Phase::Playing, Action::Play(card)) => {
                if !self.view(player).legal_cards.contains(&card) {
                    return Err(GameError(
                        "Play a card from your hand and follow the lead suit when possible.",
                    ));
                }
                self.hands[player].retain(|c| *c != card);
                self.trick.push((player, card));
                self.played.push(card);
                self.events.push(format!(
                    "{} {} {card}.",
                    NAMES[player],
                    verb(player, "play", "plays")
                ));
                if self.partner_card == Some(card) {
                    self.revealed_partner = Some(player);
                    let message = if Some(player) == self.leader {
                        format!(
                            "{} {} playing alone!",
                            NAMES[player],
                            verb(player, "are", "is")
                        )
                    } else {
                        format!(
                            "{} {} the Leader's partner!",
                            NAMES[player],
                            verb(player, "are", "is")
                        )
                    };
                    self.events.push(message);
                }
                if self.trick.len() == 4 {
                    self.finish_trick();
                } else {
                    self.turn = (player + 1) % 4;
                }
            }
            _ => return Err(GameError("That action is unavailable in this phase.")),
        }
        Ok(())
    }

    fn next_bidder(&mut self) {
        let active = self.passed.iter().filter(|passed| !**passed).count();
        if active == 0 {
            self.phase = Phase::AllPassed;
            self.events.push("Everyone passed. Redeal.".into());
        } else if active == 1 && self.leader.is_some() {
            self.finish_bidding();
        } else {
            loop {
                self.turn = (self.turn + 1) % 4;
                if !self.passed[self.turn] {
                    break;
                }
            }
        }
    }

    fn finish_bidding(&mut self) {
        self.turn = self.leader.unwrap();
        self.hands[self.turn].append(&mut self.kitty);
        sort_hand(&mut self.hands[self.turn], None);
        self.phase = Phase::Discard;
        self.events.push(format!(
            "{} {} the bid at {} and {} up the kitty.",
            NAMES[self.turn],
            verb(self.turn, "win", "wins"),
            self.high_bid,
            verb(self.turn, "pick", "picks")
        ));
    }

    fn finish_trick(&mut self) {
        let winner = trick_winner(&self.trick, self.trump.unwrap()).unwrap();
        let mut points: u16 = self.trick.iter().map(|(_, card)| card.points()).sum();
        self.tricks_finished += 1;
        if self.tricks_finished == 10 {
            points += self.kitty.iter().map(|card| card.points()).sum::<u16>();
        }
        self.captured[winner] += points;
        self.events.push(format!(
            "{} {} trick {} for {points} points{}.",
            NAMES[winner],
            verb(winner, "take", "takes"),
            self.tricks_finished,
            if self.tricks_finished == 10 {
                " (including the kitty)"
            } else {
                ""
            }
        ));
        self.last_trick = std::mem::take(&mut self.trick);
        self.last_winner = Some(winner);
        self.turn = winner;
        if self.tricks_finished == 10 {
            self.finish_round();
        }
    }

    fn finish_round(&mut self) {
        let leader = self.leader.unwrap();
        let partner = self.partner.unwrap();
        let team_points = self.captured[leader]
            + if partner != leader {
                self.captured[partner]
            } else {
                0
            };
        let made_bid = team_points >= self.high_bid;
        let scores = std::array::from_fn(|player| {
            if player == leader || player == partner {
                if made_bid {
                    i32::from(team_points)
                } else {
                    -i32::from(self.high_bid)
                }
            } else {
                i32::from(180 - team_points)
            }
        });
        self.events.push(format!(
            "Leader's team captured {team_points}/{}: {}.",
            self.high_bid,
            if made_bid { "bid made" } else { "bid missed" }
        ));
        self.result = Some(RoundResult {
            leader,
            partner,
            bid: self.high_bid,
            captured: self.captured,
            team_points,
            made_bid,
            scores,
        });
        self.phase = Phase::Complete;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{SeedableRng, rngs::StdRng};

    fn game() -> Game {
        Game::new(&mut StdRng::seed_from_u64(7))
    }

    #[test]
    fn deal_preserves_the_deck_and_private_views() {
        let game = game();
        let cards: Vec<_> = game
            .hands
            .iter()
            .flatten()
            .chain(&game.kitty)
            .copied()
            .collect();
        assert_eq!(cards.len(), 45);
        assert_eq!(cards.iter().collect::<HashSet<_>>().len(), 45);
        for player in 0..4 {
            let view = game.view(player);
            assert_eq!(view.hand.len(), 10);
            assert_eq!(view.hand, game.hands[player]);
            assert!(view.partner_choices.is_empty());
            assert!(view.kitty_has_points.is_none());
        }
    }

    #[test]
    fn auction_validates_bids_and_finishes_after_opponents_pass() {
        let mut game = game();
        let bidder = game.turn;
        for bad_bid in [0, 3, 181, 185] {
            assert!(game.apply(bidder, Action::Bid(bad_bid)).is_err());
            assert_eq!(game.high_bid, 0);
            assert_eq!(game.turn, bidder);
        }
        game.apply(bidder, Action::Bid(80)).unwrap();
        for _ in 0..3 {
            game.apply(game.turn, Action::Pass).unwrap();
        }
        assert_eq!(game.phase, Phase::Discard);
        assert_eq!(game.turn, bidder);
        assert_eq!(game.hands[bidder].len(), 15);
        assert!(game.kitty.is_empty());
    }

    #[test]
    fn all_passes_redeal_and_180_ends_bidding_immediately() {
        let mut game = game();
        for _ in 0..4 {
            game.apply(game.turn, Action::Pass).unwrap();
        }
        assert_eq!(game.phase, Phase::AllPassed);
        assert_eq!(game.turn(), None);
        let mut game = super::tests::game();
        let bidder = game.turn;
        game.apply(bidder, Action::Bid(180)).unwrap();
        assert_eq!(game.phase, Phase::Discard);
        assert_eq!(game.hands[bidder].len(), 15);
    }

    #[test]
    fn discards_are_validated_and_cannot_be_called_as_partner() {
        let mut game = game();
        let leader = game.turn;
        game.apply(leader, Action::Bid(180)).unwrap();
        let hand = game.hands[leader].clone();
        assert!(
            game.apply(leader, Action::Discard(vec![hand[0]; 5]))
                .is_err()
        );
        assert_eq!(game.hands[leader], hand);
        assert!(
            game.apply(leader, Action::Discard(hand[..4].to_vec()))
                .is_err()
        );
        game.apply(leader, Action::Discard(hand[..5].to_vec()))
            .unwrap();
        assert!(
            game.apply(
                leader,
                Action::Declare {
                    trump: Suit::Purple,
                    partner: hand[0]
                }
            )
            .is_err()
        );
        assert_eq!(game.phase, Phase::Declare);
        assert_eq!(game.view(leader).partner_choices.len(), 40);
        assert!(game.view((leader + 1) % 4).partner_choices.is_empty());
        let partner_card = hand[5];
        game.apply(
            leader,
            Action::Declare {
                trump: Suit::Purple,
                partner: partner_card,
            },
        )
        .unwrap();
        assert_eq!(game.view(leader).known_partner, Some(leader));
        assert_eq!(game.view((leader + 1) % 4).known_partner, None);
        assert_eq!(game.view(leader).revealed_partner, None);
        game.apply(leader, Action::Play(partner_card)).unwrap();
        assert_eq!(game.view((leader + 1) % 4).revealed_partner, Some(leader));
    }

    #[test]
    fn declaring_trump_moves_koor_into_its_suit_in_every_player_view() {
        for trump in Suit::ALL {
            let mut game = game();
            game.phase = Phase::Declare;
            game.turn = 0;
            game.leader = Some(0);
            game.kitty.clear();
            let hand = vec![
                Card::Number(trump, 1),
                Card::Number(trump, 11),
                Card::Koor,
                Card::Number(trump, 5),
                Card::Number(trump, 10),
            ];
            // Move Koor's owner through all four seats, including the Leader.
            for owner in 0..4 {
                game.hands = std::array::from_fn(|_| vec![]);
                game.hands[owner] = hand.clone();
                sort_hand(&mut game.hands[owner], None);
                game.phase = Phase::Declare;
                game.trump = None;
                assert_eq!(game.view(owner).hand.last(), Some(&Card::Koor));
                game.apply(
                    0,
                    Action::Declare {
                        trump,
                        partner: Card::Number(trump, 1),
                    },
                )
                .unwrap();
                assert_eq!(
                    game.view(owner).hand,
                    vec![
                        Card::Number(trump, 5),
                        Card::Number(trump, 10),
                        Card::Koor,
                        Card::Number(trump, 11),
                        Card::Number(trump, 1),
                    ]
                );
            }
        }
    }

    #[test]
    fn koor_follows_and_leads_trump_and_illegal_plays_do_not_mutate() {
        let mut game = game();
        game.phase = Phase::Playing;
        game.trump = Some(Suit::Purple);
        game.turn = 0;
        game.trick = vec![(3, Card::Number(Suit::Purple, 8))];
        game.hands[0] = vec![Card::Koor, Card::Number(Suit::Pink, 1)];
        assert_eq!(game.view(0).legal_cards, vec![Card::Koor]);
        let events = game.events.len();
        assert!(
            game.apply(0, Action::Play(Card::Number(Suit::Pink, 1)))
                .is_err()
        );
        assert_eq!(game.events.len(), events);
        assert_eq!(game.hands[0].len(), 2);
        game.trick = vec![(3, Card::Koor)];
        game.hands[0] = vec![Card::Number(Suit::Purple, 5), Card::Number(Suit::Pink, 1)];
        assert_eq!(
            game.view(0).legal_cards,
            vec![Card::Number(Suit::Purple, 5)]
        );
        game.hands[0] = vec![Card::Number(Suit::Pink, 1), Card::Number(Suit::Brown, 6)];
        assert_eq!(game.view(0).legal_cards.len(), 2);
    }

    #[test]
    fn trick_rankings_respect_lead_trump_koor_and_high_one() {
        let trump = Suit::Orange;
        assert_eq!(
            trick_winner(
                &[
                    (0, Card::Number(Suit::Pink, 5)),
                    (1, Card::Number(Suit::Brown, 1)),
                    (2, Card::Number(Suit::Pink, 14)),
                    (3, Card::Number(Suit::Pink, 1))
                ],
                trump
            ),
            Some(3)
        );
        assert_eq!(
            trick_winner(
                &[
                    (0, Card::Number(Suit::Pink, 1)),
                    (1, Card::Number(trump, 5))
                ],
                trump
            ),
            Some(1)
        );
        assert_eq!(
            trick_winner(&[(0, Card::Number(trump, 10)), (1, Card::Koor)], trump),
            Some(1)
        );
        assert_eq!(
            trick_winner(&[(0, Card::Koor), (1, Card::Number(trump, 11))], trump),
            Some(1)
        );
        assert_eq!(trick_winner(&[], trump), None);
    }

    #[test]
    fn only_last_trick_winner_takes_kitty_points() {
        let mut game = game();
        game.phase = Phase::Playing;
        game.trump = Some(Suit::Purple);
        game.leader = Some(0);
        game.partner = Some(1);
        game.high_bid = 80;
        game.tricks_finished = 9;
        game.captured = [100, 0, 0, 0];
        game.kitty = vec![
            Card::Koor,
            Card::Number(Suit::Pink, 1),
            Card::Number(Suit::Pink, 5),
            Card::Number(Suit::Pink, 6),
            Card::Number(Suit::Pink, 7),
        ];
        game.trick = vec![
            (0, Card::Number(Suit::Orange, 14)),
            (1, Card::Number(Suit::Orange, 10)),
            (2, Card::Number(Suit::Orange, 5)),
            (3, Card::Number(Suit::Orange, 1)),
        ];
        game.finish_trick();
        assert_eq!(game.captured, [100, 0, 0, 80]);
        assert_eq!(game.phase, Phase::Complete);
        assert_eq!(game.result.unwrap().team_points, 100);
    }

    #[test]
    fn team_scoring_handles_success_failure_and_playing_alone() {
        let mut game = game();
        game.leader = Some(0);
        game.partner = Some(1);
        game.high_bid = 100;
        game.captured = [20, 60, 40, 60];
        game.finish_round();
        assert_eq!(game.result.as_ref().unwrap().scores, [-100, -100, 100, 100]);
        game.captured = [30, 80, 40, 30];
        game.finish_round();
        assert_eq!(game.result.as_ref().unwrap().scores, [110, 110, 70, 70]);
        game.partner = Some(0);
        game.captured = [120, 20, 20, 20];
        game.finish_round();
        assert_eq!(game.result.as_ref().unwrap().scores, [120, 60, 60, 60]);
    }
}
