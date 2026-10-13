use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Suit {
    Purple,
    Pink,
    Orange,
    Brown,
}

impl Suit {
    pub const ALL: [Self; 4] = [Self::Purple, Self::Pink, Self::Orange, Self::Brown];

    pub fn name(self) -> &'static str {
        match self {
            Self::Purple => "Purple",
            Self::Pink => "Pink",
            Self::Orange => "Orange",
            Self::Brown => "Brown",
        }
    }
}

impl fmt::Display for Suit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Card {
    Number(Suit, u8),
    Koor,
}

impl Card {
    pub fn deck() -> Vec<Self> {
        let mut cards = Vec::with_capacity(45);
        for suit in Suit::ALL {
            for rank in (5..=14).chain(std::iter::once(1)) {
                cards.push(Self::Number(suit, rank));
            }
        }
        cards.push(Self::Koor);
        cards
    }

    pub fn suit(self, trump: Suit) -> Suit {
        match self {
            Self::Number(suit, _) => suit,
            Self::Koor => trump,
        }
    }

    // Doubled ranks leave a slot for Koor between 10 and 11.
    pub fn strength(self) -> u8 {
        match self {
            Self::Number(_, 1) => 30,
            Self::Number(_, rank) => rank * 2,
            Self::Koor => 21,
        }
    }

    pub fn points(self) -> u16 {
        match self {
            Self::Number(_, 5) => 5,
            Self::Number(_, 10 | 14) => 10,
            Self::Number(_, 1) => 15,
            Self::Koor => 20,
            _ => 0,
        }
    }

    pub fn face(self) -> String {
        match self {
            Self::Number(_, rank) => rank.to_string(),
            Self::Koor => "😛".into(),
        }
    }
}

impl fmt::Display for Card {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Number(suit, rank) => write!(f, "{suit} {rank}"),
            Self::Koor => f.write_str("Koor 😛"),
        }
    }
}

pub fn sort_hand(cards: &mut [Card], trump: Option<Suit>) {
    cards.sort_by_key(|card| match *card {
        Card::Number(suit, _) => (suit as u8, card.strength()),
        Card::Koor => (trump.map_or(4, |suit| suit as u8), card.strength()),
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn deck_matches_rules() {
        let deck = Card::deck();
        assert_eq!(deck.len(), 45);
        assert_eq!(deck.iter().collect::<HashSet<_>>().len(), 45);
        assert_eq!(deck.iter().map(|c| c.points()).sum::<u16>(), 180);
        assert!(Card::Number(Suit::Pink, 10).strength() < Card::Koor.strength());
        assert!(Card::Koor.strength() < Card::Number(Suit::Pink, 11).strength());
        assert!(Card::Number(Suit::Pink, 14).strength() < Card::Number(Suit::Pink, 1).strength());
    }

    #[test]
    fn hand_order_groups_suits_and_places_koor_in_declared_trump() {
        for trump in [None].into_iter().chain(Suit::ALL.map(Some)) {
            let mut hand = Card::deck();
            hand.reverse();
            sort_hand(&mut hand, trump);
            let mut expected = Vec::new();
            for suit in Suit::ALL {
                for rank in (5..=14).chain(std::iter::once(1)) {
                    expected.push(Card::Number(suit, rank));
                    if trump == Some(suit) && rank == 10 {
                        expected.push(Card::Koor);
                    }
                }
            }
            if trump.is_none() {
                expected.push(Card::Koor);
            }
            assert_eq!(hand, expected, "trump: {trump:?}");
        }
    }
}
