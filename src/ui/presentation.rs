use crate::cards::{Card, Suit};

pub(super) fn card_marker(
    card: Card,
    trump: Option<Suit>,
    partner_card: Option<Card>,
) -> &'static str {
    let is_trump = trump.is_some_and(|trump| card.suit(trump) == trump);
    let is_partner = partner_card == Some(card);
    match (is_trump, is_partner) {
        (true, true) => "* P",
        (true, false) => "*",
        (false, true) => "P",
        (false, false) => "",
    }
}

pub(super) fn format_log(events: &[String]) -> String {
    let mut text = String::new();
    for (index, event) in events.iter().enumerate() {
        if index > 0 {
            text.push('\n');
            // Keep each trick (including partner reveals and its result) together.
            let previous = &events[index - 1];
            if previous.contains(" takes trick ")
                || previous.contains(" take trick ")
                || previous.contains(" as partner. The kitty ")
            {
                text.push('\n');
            }
        }
        text.push_str(event);
    }
    text
}
