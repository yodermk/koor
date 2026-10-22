use super::{
    Frontend,
    presentation::{card_marker, format_log},
};
use crate::{
    cards::{Card, Suit},
    game::{Action, NAMES, Phase, View},
    session::Session,
};
use gtk4::{self as gtk, glib, prelude::*};
use std::{
    cell::RefCell,
    collections::HashSet,
    rc::Rc,
    time::{Duration, Instant},
};

pub struct GtkFrontend;

impl Frontend for GtkFrontend {
    fn run(self, session: Session) -> Result<(), String> {
        gtk::init().map_err(|error| format!("Could not initialize GTK: {error}"))?;
        let app = gtk::Application::builder()
            .application_id("io.github.koor.Koor")
            .build();
        let session = RefCell::new(Some(session));
        app.connect_activate(move |app| {
            if let Some(window) = app.active_window() {
                window.present();
                return;
            }
            let Some(session) = session.borrow_mut().take() else {
                return;
            };
            let ui = Table::new(app, session);
            ui.render();
            ui.window.present();
            glib::timeout_add_local(Duration::from_millis(100), move || {
                if !ui.window.is_visible() {
                    return glib::ControlFlow::Break;
                }
                ui.tick();
                glib::ControlFlow::Continue
            });
        });
        // Application options have already been parsed by clap.
        let code = app.run_with_args::<&str>(&[]);
        if code == glib::ExitCode::SUCCESS {
            Ok(())
        } else {
            Err(format!("GTK exited with {code:?}"))
        }
    }
}

struct Table {
    session: RefCell<Session>,
    selected: RefCell<HashSet<Card>>,
    error: RefCell<Option<String>>,
    next_bot_at: RefCell<Instant>,
    window: gtk::ApplicationWindow,
    heading: gtk::Label,
    contract: gtk::Label,
    seats: [gtk::Label; 4],
    seat_boxes: [gtk::Box; 4],
    trick: gtk::Box,
    trick_caption: gtk::Label,
    hand: gtk::FlowBox,
    controls: gtk::Box,
    scores: gtk::Label,
    log: gtk::TextView,
    status: gtk::Label,
}

fn label(text: &str, class: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.add_css_class(class);
    label.set_wrap(true);
    label
}

fn clear(container: &gtk::Box) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
}

fn card_content(card: Card, trump: Option<Suit>) -> gtk::Box {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 5);
    content.set_size_request(57, 95);
    content.append(&label(&card.face(), "rank"));
    let suit_label = match card {
        Card::Number(suit, _) => suit.name().to_owned(),
        Card::Koor => trump.map_or_else(|| "KOOR".into(), |suit| format!("KOOR\n{suit}")),
    };
    content.append(&label(&suit_label, "suit"));
    content.append(&label(&format!("{} pts", card.points()), "value"));
    content
}

fn suit_class(card: Card, trump: Option<Suit>) -> &'static str {
    let suit = match card {
        Card::Number(suit, _) => Some(suit),
        Card::Koor => trump,
    };
    match suit {
        Some(Suit::Purple) => "purple",
        Some(Suit::Pink) => "pink",
        Some(Suit::Orange) => "orange",
        Some(Suit::Brown) => "brown",
        None => "koor",
    }
}

fn suit_color(suit: Suit) -> &'static str {
    match suit {
        Suit::Purple => "#7136a2",
        Suit::Pink => "#ae2f69",
        Suit::Orange => "#a54809",
        Suit::Brown => "#704022",
    }
}

impl Table {
    fn new(app: &gtk::Application, session: Session) -> Rc<Self> {
        let provider = gtk::CssProvider::new();
        provider.load_from_data(include_str!("style.css"));
        gtk::style_context_add_provider_for_display(
            &gtk::gdk::Display::default().unwrap(),
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
        let window = gtk::ApplicationWindow::builder()
            .application(app)
            .title("Koor")
            .default_width(1180)
            .default_height(880)
            .build();
        let header = gtk::HeaderBar::new();
        let title = label("KOOR  😛", "title");
        title.set_wrap(false);
        header.set_title_widget(Some(&title));
        let rules = gtk::Button::with_label("How to play");
        let weak_window = window.downgrade();
        rules.connect_clicked(move |_| {
            if let Some(parent) = weak_window.upgrade() {
                let dialog = gtk::Window::builder().title("How to play Koor").transient_for(&parent)
                    .modal(true).default_width(640).default_height(620).build();
                let text = gtk::TextView::new();
                text.set_editable(false);
                text.set_cursor_visible(false);
                text.set_wrap_mode(gtk::WrapMode::Word);
                text.set_top_margin(20); text.set_bottom_margin(20); text.set_left_margin(20); text.set_right_margin(20);
                text.buffer().set_text(concat!(include_str!("../../RULES.md"),
                    "\n\nImplementation conventions: passing removes a player from the auction; an all-pass hand is redealt. A missed bid scores minus the bid for the Leader and partner. The opposition scores its captured points. A discarded card cannot be called as partner."));
                let scroll = gtk::ScrolledWindow::builder().child(&text).build();
                dialog.set_child(Some(&scroll));
                dialog.present();
            }
        });
        header.pack_end(&rules);
        window.set_titlebar(Some(&header));

        let outer = gtk::Box::new(gtk::Orientation::Horizontal, 20);
        outer.set_margin_top(20);
        outer.set_margin_bottom(20);
        outer.set_margin_start(20);
        outer.set_margin_end(20);
        let main = gtk::Box::new(gtk::Orientation::Vertical, 16);
        main.set_hexpand(true);
        let heading = label("", "heading");
        heading.set_xalign(0.0);
        let contract = label("", "muted");
        contract.set_xalign(0.0);
        main.append(&heading);
        main.append(&contract);

        let table = gtk::Box::new(gtk::Orientation::Vertical, 20);
        table.add_css_class("table");
        table.set_vexpand(true);
        let seats_row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        let seats = std::array::from_fn(|_| {
            let seat = label("", "score");
            seat.set_wrap(false);
            seat
        });
        let seat_boxes = std::array::from_fn(|index| {
            let seat = gtk::Box::new(gtk::Orientation::Vertical, 0);
            seat.set_hexpand(true);
            seat.add_css_class("seat");
            seat.append(&seats[index]);
            seats_row.append(&seat);
            seat
        });
        table.append(&seats_row);
        let trick_caption = label("", "muted");
        table.append(&trick_caption);
        let trick = gtk::Box::new(gtk::Orientation::Horizontal, 16);
        trick.set_halign(gtk::Align::Center);
        trick.set_valign(gtk::Align::Center);
        trick.set_vexpand(true);
        table.append(&trick);
        main.append(&table);

        let controls = gtk::Box::new(gtk::Orientation::Vertical, 10);
        controls.add_css_class("controls");
        main.append(&controls);
        let hand_title = label("YOUR HAND  ·  Click a card to play", "caption");
        hand_title.set_xalign(0.0);
        main.append(&hand_title);
        let hand = gtk::FlowBox::new();
        hand.set_selection_mode(gtk::SelectionMode::None);
        hand.set_homogeneous(true);
        hand.set_column_spacing(8);
        hand.set_row_spacing(8);
        hand.set_min_children_per_line(5);
        hand.set_max_children_per_line(10);
        let hand_scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .min_content_height(150)
            .max_content_height(300)
            .propagate_natural_height(true)
            .child(&hand)
            .build();
        main.append(&hand_scroll);
        let status = label("", "caption");
        status.set_xalign(0.0);
        main.append(&status);
        let main_scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .hexpand(true)
            .vexpand(true)
            .child(&main)
            .build();
        outer.append(&main_scroll);

        let sidebar = gtk::Box::new(gtk::Orientation::Vertical, 16);
        sidebar.add_css_class("sidebar");
        sidebar.set_size_request(265, -1);
        sidebar.append(&label("SCOREBOARD", "caption"));
        let scores = label("", "score");
        scores.set_xalign(0.0);
        sidebar.append(&scores);
        sidebar.append(&label("AT THE TABLE", "caption"));
        let log = gtk::TextView::new();
        log.set_editable(false);
        log.set_cursor_visible(false);
        log.set_wrap_mode(gtk::WrapMode::Word);
        let scroll = gtk::ScrolledWindow::builder()
            .vexpand(true)
            .child(&log)
            .build();
        sidebar.append(&scroll);
        outer.append(&sidebar);
        window.set_child(Some(&outer));
        Rc::new(Self {
            session: RefCell::new(session),
            selected: RefCell::new(HashSet::new()),
            error: RefCell::new(None),
            next_bot_at: RefCell::new(Instant::now() + Duration::from_millis(700)),
            window,
            heading,
            contract,
            seats,
            seat_boxes,
            trick,
            trick_caption,
            hand,
            controls,
            scores,
            log,
            status,
        })
    }

    fn tick(self: &Rc<Self>) {
        if Instant::now() < *self.next_bot_at.borrow() {
            return;
        }
        if self.session.borrow().game.phase() == Phase::AllPassed {
            let result = self.session.borrow_mut().next_round();
            if let Err(error) = result {
                *self.error.borrow_mut() = Some(error.to_string());
            }
            self.render();
            *self.next_bot_at.borrow_mut() = Instant::now() + Duration::from_millis(700);
            return;
        }
        let before = self.session.borrow().view(0).tricks_finished;
        let result = self.session.borrow_mut().step_bot();
        match result {
            Ok(true) => {
                let finished = self.session.borrow().view(0).tricks_finished > before;
                self.render();
                *self.next_bot_at.borrow_mut() =
                    Instant::now() + Duration::from_millis(if finished { 1700 } else { 650 });
            }
            Ok(false) => {}
            Err(error) => {
                *self.error.borrow_mut() = Some(error.to_string());
                self.render();
            }
        }
    }

    fn act(self: &Rc<Self>, action: Action) {
        let before = self.session.borrow().view(0).tricks_finished;
        let result = self.session.borrow_mut().act(0, action);
        *self.error.borrow_mut() = result.err().map(|error| error.to_string());
        let finished = self.session.borrow().view(0).tricks_finished > before;
        if self.error.borrow().is_none() {
            self.selected.borrow_mut().clear();
        }
        *self.next_bot_at.borrow_mut() =
            Instant::now() + Duration::from_millis(if finished { 1700 } else { 650 });
        self.render();
    }

    fn action_button(self: &Rc<Self>, title: &str, action: Action, primary: bool) -> gtk::Button {
        let button = gtk::Button::with_label(title);
        if primary {
            button.add_css_class("suggested-action");
        }
        let weak = Rc::downgrade(self);
        button.connect_clicked(move |_| {
            if let Some(ui) = weak.upgrade() {
                ui.act(action.clone());
            }
        });
        button
    }

    fn render(self: &Rc<Self>) {
        let session = self.session.borrow();
        let view = session.view(0);
        let your_turn = view.turn == Some(0);
        let phase = match view.phase {
            Phase::Bidding => "Bidding",
            Phase::Discard => "Choose five discards",
            Phase::Declare => "Choose trump & partner",
            Phase::Playing => "Play a card",
            Phase::Complete => "Round complete",
            Phase::AllPassed => "Redealing",
        };
        self.heading
            .set_text(&format!("Round {}  ·  {phase}", session.round));
        self.contract
            .set_markup(&if let (Some(trump), Some(partner), Some(leader)) =
                (view.trump, view.partner_card, view.leader)
            {
                let (kitty_text, kitty_color) = if view.kitty_has_points == Some(true) {
                    ("has points", "#24663c")
                } else {
                    ("has no points", "#923b3b")
                };
                let contract = glib::markup_escape_text(&format!(
                    "{} bid {}  ·  ",
                    NAMES[leader], view.high_bid
                ));
                let partner_text = glib::markup_escape_text(&format!("Partner: {partner}"));
                let team_scores = view.known_partner.map_or_else(String::new, |partner| {
                    let leader_points = view.captured[leader]
                        + if partner == leader { 0 } else { view.captured[partner] };
                    let opposition_points: u16 = view.captured.iter().enumerate()
                        .filter(|(player, _)| *player != leader && *player != partner)
                        .map(|(_, points)| *points)
                        .sum();
                    format!("  ·  Leader/Partner: {leader_points} pts  ·  Opposition: {opposition_points} pts")
                });
                format!(
                    "{contract}<span background=\"#faf5ea\" foreground=\"{}\" weight=\"bold\"> {trump} trump </span>  ·  {partner_text}  ·  <span background=\"{kitty_color}\" foreground=\"#ffffff\" weight=\"bold\">Kitty {kitty_text}</span>{team_scores}",
                    suit_color(trump)
                )
            } else {
                format!(
                    "High bid: {}  ·  180 points in every hand  ·  Play proceeds clockwise",
                    view.high_bid
                )
            });
        for player in 0..4 {
            let role = if view.leader == Some(player) {
                " · Leader"
            } else if view.revealed_partner == Some(player) {
                " · Partner"
            } else {
                ""
            };
            let position = ["bottom", "left", "across", "right"][player];
            self.seats[player].set_text(&format!(
                "{} · {} pts{role}\n{} cards · {position}",
                NAMES[player], view.captured[player], view.hand_sizes[player]
            ));
            if view.turn == Some(player) {
                self.seat_boxes[player].add_css_class("active");
            } else {
                self.seat_boxes[player].remove_css_class("active");
            }
        }
        self.scores.set_text(
            &(0..4)
                .map(|p| {
                    format!(
                        "{}     {}\n{} pts this hand",
                        NAMES[p], session.totals[p], view.captured[p]
                    )
                })
                .collect::<Vec<_>>()
                .join("\n\n"),
        );
        self.log.buffer().set_text(&format_log(&view.events));
        let mut end = self.log.buffer().end_iter();
        self.log.scroll_to_iter(&mut end, 0.0, false, 0.0, 1.0);
        self.status.set_text(&if let Some(error) = self.error.borrow().as_ref() { error.clone() }
            else if let Some(warning) = &session.warning { warning.clone() }
            else { format!("{} tricks played  ·  {} learning rounds  ·  Koor ranks between 10 and 11 of trump", view.tricks_finished, session.learning.rounds) });
        if self.error.borrow().is_some() || session.warning.is_some() {
            self.status.add_css_class("error");
        } else {
            self.status.remove_css_class("error");
        }
        drop(session);

        clear(&self.trick);
        let show_previous = view.trick.is_empty() && !view.last_trick.is_empty();
        let displayed = if show_previous {
            &view.last_trick
        } else {
            &view.trick
        };
        self.trick_caption.set_text(&if show_previous {
            format!("{} took the last trick", NAMES[view.last_winner.unwrap()])
        } else {
            format!("Trick {} of 10", (view.tricks_finished + 1).min(10))
        });
        for (player, name) in NAMES.iter().enumerate() {
            let slot = gtk::Box::new(gtk::Orientation::Vertical, 8);
            slot.append(&label(name, "caption"));
            if let Some((_, card)) = displayed.iter().find(|(p, _)| *p == player) {
                let content = card_content(*card, view.trump);
                content.add_css_class("card");
                content.add_css_class(suit_class(*card, view.trump));
                slot.append(&content);
            } else {
                let empty = label("·", "muted");
                empty.set_size_request(80, 115);
                slot.append(&empty);
            }
            self.trick.append(&slot);
        }
        while let Some(child) = self.hand.first_child() {
            self.hand.remove(&child);
        }
        for card in &view.hand {
            let card = *card;
            let button = gtk::Button::new();
            let content = card_content(card, view.trump);
            let marker = label(
                card_marker(card, view.trump, view.partner_card),
                "trump-marker",
            );
            marker.set_vexpand(true);
            marker.set_valign(gtk::Align::End);
            content.append(&marker);
            button.set_child(Some(&content));
            button.add_css_class("card");
            button.add_css_class(suit_class(card, view.trump));
            button.set_tooltip_text(Some(&format!("{card} · {} points", card.points())));
            if self.selected.borrow().contains(&card) {
                button.add_css_class("selected");
            }
            let discard = view.phase == Phase::Discard && your_turn;
            button.set_sensitive(discard || view.legal_cards.contains(&card));
            let weak = Rc::downgrade(self);
            button.connect_clicked(move |_| {
                if let Some(ui) = weak.upgrade() {
                    if discard {
                        let mut selected = ui.selected.borrow_mut();
                        if !selected.remove(&card) && selected.len() < 5 {
                            selected.insert(card);
                        }
                        drop(selected);
                        ui.render();
                    } else {
                        ui.act(Action::Play(card));
                    }
                }
            });
            self.hand.insert(&button, -1);
        }
        self.render_controls(&view);
    }

    fn render_controls(self: &Rc<Self>, view: &View) {
        clear(&self.controls);
        if view.phase == Phase::Complete {
            let result = view.result.as_ref().unwrap();
            let partner = if result.partner == result.leader {
                "playing alone".into()
            } else {
                format!("with {}", NAMES[result.partner])
            };
            self.controls.append(&label(
                &format!(
                    "{} {partner}: {} points against a bid of {}. {}",
                    NAMES[result.leader],
                    result.team_points,
                    result.bid,
                    if result.made_bid {
                        "Bid made!"
                    } else {
                        "Bid missed. Leader's team loses the bid amount."
                    }
                ),
                "score",
            ));
            let button = gtk::Button::with_label("Deal next round");
            button.add_css_class("suggested-action");
            let weak = Rc::downgrade(self);
            button.connect_clicked(move |_| {
                if let Some(ui) = weak.upgrade() {
                    let result = ui.session.borrow_mut().next_round();
                    *ui.error.borrow_mut() = result.err().map(|e| e.to_string());
                    ui.selected.borrow_mut().clear();
                    *ui.next_bot_at.borrow_mut() = Instant::now() + Duration::from_millis(650);
                    ui.render();
                }
            });
            self.controls.append(&button);
            return;
        }
        if view.turn != Some(0) {
            self.controls.append(&label(
                &view
                    .turn
                    .map(|p| format!("{} is thinking…", NAMES[p]))
                    .unwrap_or_else(|| "Everyone passed. Shuffling a new hand…".into()),
                "muted",
            ));
            return;
        }
        match view.phase {
            Phase::Bidding => {
                self.controls.append(&label(
                    "Raise the bid or pass. Passing ends your participation in this auction.",
                    "muted",
                ));
                let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
                let spin = gtk::SpinButton::with_range(f64::from(view.high_bid + 5), 180.0, 5.0);
                spin.set_value(f64::from((view.high_bid + 5).max(80)));
                spin.set_numeric(true);
                spin.set_snap_to_ticks(true);
                spin.set_tooltip_text(Some("Bid in multiples of five"));
                row.append(&spin);
                let bid = gtk::Button::with_label("Bid");
                bid.add_css_class("suggested-action");
                let weak = Rc::downgrade(self);
                bid.connect_clicked(move |_| {
                    if let Some(ui) = weak.upgrade() {
                        spin.update();
                        ui.act(Action::Bid(spin.value_as_int() as u16));
                    }
                });
                row.append(&bid);
                row.append(&self.action_button("Pass", Action::Pass, false));
                self.controls.append(&row);
            }
            Phase::Discard => {
                let selected = self.selected.borrow();
                self.controls.append(&label(
                    &format!(
                        "Select five cards to return to the kitty. {} / 5 selected.",
                        selected.len()
                    ),
                    "muted",
                ));
                let button = self.action_button(
                    "Discard selected cards",
                    Action::Discard(selected.iter().copied().collect()),
                    true,
                );
                button.set_sensitive(selected.len() == 5);
                self.controls.append(&button);
            }
            Phase::Declare => {
                self.controls.append(&label("Call a partner card. Calling a card in your hand means playing alone, kept secret until that card is played.", "muted"));
                let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
                row.append(&label("Trump", "caption"));
                let trump = gtk::DropDown::from_strings(&Suit::ALL.map(Suit::name));
                let suggested = crate::ai::choose_action(view, crate::ai::Style::Balanced);
                if let Action::Declare { trump: suit, .. } = suggested {
                    trump.set_selected(suit as u32);
                }
                row.append(&trump);
                row.append(&label("Partner", "caption"));
                let names: Vec<_> = view
                    .partner_choices
                    .iter()
                    .map(|c| {
                        format!(
                            "{c}{}",
                            if view.hand.contains(c) {
                                " (your card)"
                            } else {
                                ""
                            }
                        )
                    })
                    .collect();
                let names: Vec<_> = names.iter().map(String::as_str).collect();
                let partner = gtk::DropDown::from_strings(&names);
                if let Action::Declare { partner: card, .. } = suggested
                    && let Some(index) = view.partner_choices.iter().position(|c| *c == card)
                {
                    partner.set_selected(index as u32);
                }
                row.append(&partner);
                let choices = view.partner_choices.clone();
                let button = gtk::Button::with_label("Announce");
                button.add_css_class("suggested-action");
                let weak = Rc::downgrade(self);
                button.connect_clicked(move |_| {
                    if let Some(ui) = weak.upgrade()
                        && let (Some(suit), Some(card)) = (
                            Suit::ALL.get(trump.selected() as usize),
                            choices.get(partner.selected() as usize),
                        )
                    {
                        ui.act(Action::Declare {
                            trump: *suit,
                            partner: *card,
                        });
                    }
                });
                row.append(&button);
                self.controls.append(&row);
            }
            Phase::Playing => {
                let lead = view.trick.first().map(|(_, c)| c.suit(view.trump.unwrap()));
                let mut prompt = lead
                    .map(|s| format!("Your turn. Follow {s} if you can. Click a highlighted card."))
                    .unwrap_or_else(|| "You lead. Click any card in your hand.".into());
                if view.known_partner == Some(0) && view.revealed_partner.is_none() {
                    prompt.push_str(if view.leader == Some(0) {
                        " You are secretly playing alone."
                    } else {
                        " You hold the partner card!"
                    });
                }
                self.controls.append(&label(&prompt, "muted"));
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn descendants(widget: &impl IsA<gtk::Widget>) -> Vec<gtk::Widget> {
        let mut result = vec![];
        let mut child = widget.first_child();
        while let Some(current) = child {
            result.push(current.clone());
            result.extend(descendants(&current));
            child = current.next_sibling();
        }
        result
    }

    fn click_control(ui: &Table, title: &str) {
        let button = descendants(&ui.controls)
            .into_iter()
            .filter_map(|w| w.downcast::<gtk::Button>().ok())
            .find(|b| b.label().as_deref() == Some(title))
            .expect("control button exists");
        assert!(button.is_sensitive());
        button.emit_clicked();
    }

    fn click_card(ui: &Table, card: Card) {
        let tooltip = format!("{card} · {} points", card.points());
        let button = descendants(&ui.hand)
            .into_iter()
            .filter_map(|w| w.downcast::<gtk::Button>().ok())
            .find(|b| b.tooltip_text().as_deref() == Some(&tooltip))
            .expect("hand card exists");
        assert!(button.is_sensitive());
        button.emit_clicked();
    }

    fn pump_events() {
        let context = glib::MainContext::default();
        for _ in 0..20 {
            while context.pending() {
                context.iteration(false);
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn assert_suit_colors(ui: &Table) {
        for (suit, hex) in [
            (Suit::Purple, "#7136a2"),
            (Suit::Pink, "#ae2f69"),
            (Suit::Orange, "#a54809"),
            (Suit::Brown, "#704022"),
        ] {
            for (card, trump) in [(Card::Number(suit, 10), None), (Card::Koor, Some(suit))] {
                let content = card_content(card, trump);
                let button = gtk::Button::new();
                button.set_child(Some(&content));
                button.add_css_class("card");
                button.add_css_class(suit_class(card, trump));
                ui.controls.append(&button);
                let expected = gtk::gdk::RGBA::parse(hex).unwrap();
                for sensitive in [true, false] {
                    button.set_sensitive(sensitive);
                    for selected in [false, true] {
                        if selected {
                            button.add_css_class("selected");
                        } else {
                            button.remove_css_class("selected");
                        }
                        for child in descendants(&button) {
                            if child.has_css_class("rank") || child.has_css_class("suit") {
                                assert_eq!(
                                    child.style_context().color(),
                                    expected,
                                    "{card}, trump: {trump:?}"
                                );
                            }
                        }
                    }
                }
                ui.controls.remove(&button);
                button.set_child(None::<&gtk::Widget>);
                // Played cards use a box instead of a button.
                content.add_css_class("card");
                content.add_css_class(suit_class(card, trump));
                ui.controls.append(&content);
                for child in descendants(&content) {
                    if child.has_css_class("rank") || child.has_css_class("suit") {
                        assert_eq!(child.style_context().color(), expected);
                    }
                }
                ui.controls.remove(&content);
            }
        }
    }

    fn screenshot(ui: &Table, name: &str) {
        if let Some(directory) = std::env::var_os("KOOR_SCREENSHOT_DIR") {
            pump_events();
            // Broadway has no frame feedback when there is no interactive client.
            ui.window
                .allocate(ui.window.width(), ui.window.height(), -1, None);
            let snapshot = gtk::Snapshot::new();
            snapshot.append_color(
                &gtk::gdk::RGBA::parse("#111b20").unwrap(),
                &gtk::graphene::Rect::new(
                    0.0,
                    0.0,
                    ui.window.width() as f32,
                    ui.window.height() as f32,
                ),
            );
            if let Some(titlebar) = ui.window.titlebar() {
                ui.window.snapshot_child(&titlebar, &snapshot);
            }
            ui.window
                .snapshot_child(&ui.window.child().unwrap(), &snapshot);
            let node = snapshot.to_node().unwrap_or_else(|| {
                panic!(
                    "{name}: window has no rendered content ({}×{}, mapped={})",
                    ui.window.width(),
                    ui.window.height(),
                    ui.window.is_mapped()
                )
            });
            let texture = ui.window.renderer().unwrap().render_texture(&node, None);
            texture
                .save_to_png(std::path::PathBuf::from(directory).join(format!("{name}.png")))
                .unwrap();
        }
    }

    #[test]
    #[ignore = "requires a display; run with GDK_BACKEND=broadway or on a desktop"]
    fn gtk_controls_play_a_complete_round() {
        gtk::init().expect("GTK display available");
        gtk::Settings::default()
            .unwrap()
            .set_gtk_enable_animations(false);
        let app = gtk::Application::builder()
            .application_id("io.github.koor.SmokeTest")
            .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
            .build();
        app.register(None::<&gtk::gio::Cancellable>).unwrap();
        let ui = Table::new(&app, Session::new(42, Some(0), None).unwrap());
        assert_suit_colors(&ui);
        ui.render();
        ui.window.present();
        pump_events();
        let mut saw_discard = false;
        let mut saw_declare = false;
        let mut human_plays = 0;
        for _ in 0..200 {
            let view = ui.session.borrow().view(0);
            if view.phase == Phase::Complete {
                break;
            }
            if view.turn != Some(0) {
                assert!(ui.session.borrow_mut().step_bot().unwrap());
                ui.render();
                continue;
            }
            match view.phase {
                Phase::Bidding => {
                    screenshot(&ui, "bidding");
                    let spin = descendants(&ui.controls)
                        .into_iter()
                        .find_map(|w| w.downcast::<gtk::SpinButton>().ok())
                        .unwrap();
                    spin.set_value(180.0);
                    click_control(&ui, "Bid");
                }
                Phase::Discard => {
                    saw_discard = true;
                    for card in crate::ai::choose_discards(&view.hand) {
                        click_card(&ui, card);
                    }
                    screenshot(&ui, "discard");
                    click_control(&ui, "Discard selected cards");
                }
                Phase::Declare => {
                    saw_declare = true;
                    let Action::Declare { trump, partner } =
                        crate::ai::choose_action(&view, crate::ai::Style::Balanced)
                    else {
                        unreachable!()
                    };
                    let dropdowns: Vec<_> = descendants(&ui.controls)
                        .into_iter()
                        .filter_map(|w| w.downcast::<gtk::DropDown>().ok())
                        .collect();
                    dropdowns[0].set_selected(trump as u32);
                    dropdowns[1].set_selected(
                        view.partner_choices
                            .iter()
                            .position(|c| *c == partner)
                            .unwrap() as u32,
                    );
                    click_control(&ui, "Announce");
                    screenshot(&ui, "playing");
                }
                Phase::Playing => {
                    let Action::Play(card) =
                        crate::ai::choose_action(&view, crate::ai::Style::Balanced)
                    else {
                        unreachable!()
                    };
                    click_card(&ui, card);
                    human_plays += 1;
                }
                _ => panic!("Unexpected phase"),
            }
            assert!(ui.error.borrow().is_none());
        }
        assert!(saw_discard && saw_declare);
        assert_eq!(human_plays, 10);
        assert_eq!(ui.session.borrow().game.phase(), Phase::Complete);
        screenshot(&ui, "complete");
        click_control(&ui, "Deal next round");
        assert_eq!(ui.session.borrow().round, 2);
        assert_eq!(ui.session.borrow().game.phase(), Phase::Bidding);
        ui.window.close();
        pump_events();
    }
}
