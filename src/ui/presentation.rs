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

#[cfg(test)]
mod tests {
    #[test]
    fn log_keeps_plays_and_partner_reveals_in_their_trick() {
        let events = [
            "You bid 120.",
            "Ada passes.",
            "You discard five cards face down.",
            "You call Pink trump and Pink 1 as partner. The kitty has points.",
            "You play Purple 5.",
            "Ada plays Pink 1.",
            "Ada is revealed as your partner.",
            "Linus plays Purple 6.",
            "Grace plays Purple 7.",
            "Ada takes trick 1 for 20 points.",
            "Ada plays Pink 14.",
            "You play Koor 😛.",
            "You take trick 2 for 30 points.",
            "You play Pink 10.",
        ]
        .map(String::from);
        assert_eq!(
            super::format_log(&events),
            concat!(
                "You bid 120.\nAda passes.\nYou discard five cards face down.\n",
                "You call Pink trump and Pink 1 as partner. The kitty has points.\n\n",
                "You play Purple 5.\nAda plays Pink 1.\nAda is revealed as your partner.\n",
                "Linus plays Purple 6.\nGrace plays Purple 7.\nAda takes trick 1 for 20 points.\n\n",
                "Ada plays Pink 14.\nYou play Koor 😛.\nYou take trick 2 for 30 points.\n\n",
                "You play Pink 10.",
            )
        );
    }
}
