use crate::{
    cards::{Card, Suit},
    game::{Action, Phase, Player, View, trick_winner},
};
use rand::Rng;

#[derive(Clone, Copy, Debug)]
pub enum Style {
    Careful,
    Balanced,
    Bold,
}

impl Style {
    pub fn from_index(index: usize) -> Self {
        match index {
            0 => Self::Careful,
            1 => Self::Balanced,
            _ => Self::Bold,
        }
    }

    fn bid_adjustment(self) -> i32 {
        match self {
            Self::Careful => -15,
            Self::Balanced => 0,
            Self::Bold => 15,
        }
    }
}

pub fn preferred_trump(hand: &[Card]) -> Suit {
    Suit::ALL
        .into_iter()
        .max_by_key(|suit| {
            hand.iter()
                .filter(|card| card.suit(*suit) == *suit)
                .map(|card| {
                    12 + i32::from(card.strength()) + if card.strength() >= 22 { 12 } else { 0 }
                })
                .sum::<i32>()
        })
        .unwrap()
}

pub fn estimate_bid(hand: &[Card]) -> u16 {
    let trump = preferred_trump(hand);
    let count = hand.iter().filter(|card| card.suit(trump) == trump).count() as i32;
    let controls = hand
        .iter()
        .map(|card| match card {
            Card::Number(suit, 1) => {
                if *suit == trump {
                    18
                } else {
                    10 + if hand.contains(&Card::Number(*suit, 14)) {
                        5
                    } else {
                        0
                    }
                }
            }
            Card::Number(suit, 11..=14) if *suit == trump => 10,
            Card::Koor => 7,
            _ => 0,
        })
        .sum::<i32>();
    // The kitty and a called partner usually improve the initial ten-card hand.
    ((55 + count * 10 + controls).clamp(80, 170) / 5 * 5) as u16
}

/// Sample once per hand so a bot's appetite stays consistent during the auction.
pub(crate) fn bid_ceiling(hand: &[Card], style: Style, rng: &mut impl Rng) -> u16 {
    // Favor small adjustments, with occasional excursions of ten points either way.
    let jitter = (rng.random_range(-1..=1) + rng.random_range(-1..=1)) * 5;
    (i32::from(estimate_bid(hand)) + style.bid_adjustment() + jitter).clamp(5, 180) as u16
}

/// No access to the engine or opponents' hands, even during training.
pub fn choose_action(view: &View, style: Style) -> Action {
    let ceiling = if view.phase == Phase::Bidding {
        (i32::from(estimate_bid(&view.hand)) + style.bid_adjustment()).clamp(5, 180) as u16
    } else {
        0
    };
    choose_action_with_bid_ceiling(view, style, ceiling)
}

pub(crate) fn choose_action_with_bid_ceiling(view: &View, style: Style, ceiling: u16) -> Action {
    match view.phase {
        Phase::Bidding => {
            if view.high_bid + 5 <= ceiling {
                Action::Bid((view.high_bid + 5).max(80.min(ceiling)))
            } else {
                Action::Pass
            }
        }
        Phase::Discard => Action::Discard(choose_discards(&view.hand)),
        Phase::Declare => {
            let trump = preferred_trump(&view.hand);
            let partner = choose_partner(view, trump, style);
            Action::Declare { trump, partner }
        }
        Phase::Playing => Action::Play(choose_play(view, style)),
        Phase::Complete | Phase::AllPassed => panic!("No action in a finished round"),
    }
}

pub fn choose_discards(hand: &[Card]) -> Vec<Card> {
    let trump = preferred_trump(hand);
    let mut best = (i32::MIN, 0_u32);
    // Only 3,003 possible discards. Prefer controls and short/void side suits.
    for mask in 0_u32..(1 << hand.len()) {
        if mask.count_ones() != 5 {
            continue;
        }
        let mut lengths = [0; 4];
        let mut score = 0;
        for (index, card) in hand.iter().enumerate() {
            if mask & (1 << index) != 0 {
                continue;
            }
            let suit = card.suit(trump);
            lengths[suit as usize] += 1;
            score += match card {
                Card::Number(_, 1) => 120,
                Card::Koor => 65,
                _ if suit == trump => 35 + i32::from(card.strength()) * 2,
                Card::Number(suit, 14) if hand.contains(&Card::Number(*suit, 1)) => 60,
                _ => i32::from(card.strength()) - i32::from(card.points()),
            };
        }
        for suit in Suit::ALL {
            if suit != trump {
                score += match lengths[suit as usize] {
                    0 => 55,
                    1 => -15,
                    _ => 0,
                };
            }
        }
        if score > best.0 {
            best = (score, mask);
        }
    }
    hand.iter()
        .enumerate()
        .filter(|(index, _)| best.1 & (1 << index) != 0)
        .map(|(_, card)| *card)
        .collect()
}

fn choose_partner(view: &View, trump: Suit, style: Style) -> Card {
    let ace = Card::Number(trump, 1);
    if !view.hand.contains(&ace) && view.partner_choices.contains(&ace) {
        return ace;
    }
    let trumps = view
        .hand
        .iter()
        .filter(|card| card.suit(trump) == trump)
        .count();
    if trumps >= 6
        && view.hand.contains(&ace)
        && matches!(style, Style::Bold)
        && view.high_bid <= 135
    {
        return ace;
    }
    for suit in Suit::ALL {
        let card = Card::Number(suit, 1);
        if suit != trump
            && view.hand.contains(&Card::Number(suit, 14))
            && !view.hand.contains(&card)
            && view.partner_choices.contains(&card)
        {
            return card;
        }
    }
    view.partner_choices
        .iter()
        .copied()
        .filter(|card| !view.hand.contains(card))
        .max_by_key(|card| (card.suit(trump) == trump, card.strength()))
        .unwrap_or(ace)
}

fn allied(view: &View, other: Player) -> bool {
    if other == view.player {
        return true;
    }
    match (view.leader, view.known_partner) {
        (Some(leader), Some(partner)) => {
            let on_team = |player| player == leader || player == partner;
            on_team(view.player) == on_team(other)
        }
        _ => false,
    }
}

fn choose_play(view: &View, style: Style) -> Card {
    let trump = view.trump.unwrap();
    let legal = &view.legal_cards;
    if view.trick.is_empty() {
        // A control is the highest remaining card in its suit, using public history.
        return *legal
            .iter()
            .max_by_key(|card| {
                let suit = card.suit(trump);
                let control = Card::deck()
                    .into_iter()
                    .filter(|c| c.suit(trump) == suit && c.strength() > card.strength())
                    .all(|c| view.played.contains(&c) || view.hand.contains(&c));
                let draw_trump =
                    view.leader == Some(view.player) && view.tricks_finished < 4 && suit == trump;
                let mut score = if control { 100 } else { 0 };
                if control && draw_trump {
                    score += 60;
                }
                if control {
                    score += i32::from(card.strength());
                } else {
                    score -= i32::from(card.points()) * 2 + i32::from(card.strength());
                }
                if matches!(style, Style::Careful) && suit == trump && !control {
                    score -= 20;
                }
                score
            })
            .unwrap();
    }
    let winner = trick_winner(&view.trick, trump).unwrap();
    let team_winning = allied(view, winner);
    let trick_points = view.trick.iter().map(|(_, c)| c.points()).sum::<u16>();
    *legal
        .iter()
        .max_by_key(|card| {
            let mut hypothetical = view.trick.clone();
            hypothetical.push((view.player, **card));
            let wins = trick_winner(&hypothetical, trump) == Some(view.player);
            // Feed a known teammate, otherwise buy a trick with the cheapest winner.
            if team_winning {
                i32::from(card.points()) * 5 - i32::from(card.strength())
            } else if wins {
                100 + i32::from(trick_points) * 2 - i32::from(card.strength())
            } else {
                -i32::from(card.points()) * 5 - i32::from(card.strength())
            }
        })
        .unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::Game;
    use rand::{SeedableRng, rngs::StdRng};
    use std::collections::HashSet;

    #[test]
    fn high_trumps_and_side_suit_controls_support_higher_bids() {
        let mut hand: Vec<_> = (5..=9)
            .map(|rank| Card::Number(Suit::Purple, rank))
            .chain((5..=7).map(|rank| Card::Number(Suit::Pink, rank)))
            .chain([Card::Number(Suit::Brown, 5), Card::Number(Suit::Orange, 5)])
            .collect();
        let low_trumps = estimate_bid(&hand);
        assert!((100..=110).contains(&low_trumps));
        for (card, rank) in hand[..5].iter_mut().zip([11, 12, 13, 14, 1]) {
            *card = Card::Number(Suit::Purple, rank);
        }
        assert!(estimate_bid(&hand) >= 140);
        assert!(estimate_bid(&hand) >= low_trumps + 30);

        // An ace/14 pair is worth more than the same ace with a low card.
        hand[0] = Card::Number(Suit::Purple, 5);
        hand[5] = Card::Number(Suit::Pink, 1);
        let side_ace = estimate_bid(&hand);
        hand[6] = Card::Number(Suit::Pink, 14);
        assert!(estimate_bid(&hand) >= side_ace + 5);
    }

    #[test]
    fn bidding_varies_within_bounds_and_always_uses_legal_increments() {
        let mut rng = StdRng::seed_from_u64(42);
        let game = Game::new(&mut rng);
        let mut view = game.view(game.turn().unwrap());
        let ordinary_hand = view.hand.clone();
        let strong_hand: Vec<_> = (6..=14)
            .chain([1])
            .map(|rank| Card::Number(Suit::Purple, rank))
            .collect();
        for hand in [ordinary_hand, strong_hand] {
            view.hand = hand;
            for style in [Style::Careful, Style::Balanced, Style::Bold] {
                let base = i32::from(estimate_bid(&view.hand)) + style.bid_adjustment();
                let mut ceilings = HashSet::new();
                for _ in 0..128 {
                    let ceiling = bid_ceiling(&view.hand, style, &mut rng);
                    assert!(
                        ((base - 10).max(5)..=(base + 10).min(180)).contains(&i32::from(ceiling))
                    );
                    assert!(ceiling.is_multiple_of(5));
                    ceilings.insert(ceiling);
                    for high_bid in (0..=180).step_by(5) {
                        view.high_bid = high_bid;
                        match choose_action_with_bid_ceiling(&view, style, ceiling) {
                            Action::Bid(bid) => {
                                assert!(bid > high_bid && bid <= ceiling && bid <= 180);
                                assert!(bid.is_multiple_of(5));
                            }
                            Action::Pass => assert!(high_bid + 5 > ceiling),
                            action => panic!("Unexpected bidding action: {action:?}"),
                        }
                    }
                }
                let possible_ceilings = ((base + 10).min(180) - (base - 10).max(5)) / 5 + 1;
                assert_eq!(ceilings.len(), possible_ceilings as usize);
            }
        }
    }
}
