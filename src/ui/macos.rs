use super::{
    Frontend,
    presentation::{card_marker, format_log},
};
use crate::{
    ai::{self, Style},
    cards::{Card, Suit},
    game::{Action, NAMES, Phase, View},
    session::Session,
};
use objc2::{
    AnyThread, DefinedClass, MainThreadOnly, define_class, msg_send,
    rc::{Retained, autoreleasepool},
    runtime::{AnyObject, ProtocolObject},
    sel,
};
use objc2_app_kit::*;
use objc2_foundation::{
    MainThreadMarker, NSArray, NSAttributedString, NSDictionary, NSEdgeInsets,
    NSMutableAttributedString, NSNotification, NSObject, NSObjectProtocol, NSPoint, NSRange,
    NSRect, NSSize, NSString, NSTimer, ns_string,
};
use std::{
    cell::{Cell, OnceCell, RefCell},
    collections::HashSet,
    time::{Duration, Instant},
};

pub struct MacFrontend;

impl Frontend for MacFrontend {
    fn run(self, session: Session) -> Result<(), String> {
        let mtm = MainThreadMarker::new().ok_or("AppKit must run on the main thread.")?;
        autoreleasepool(|_| {
            let app = NSApplication::sharedApplication(mtm);
            app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
            let controller = Controller::new(mtm, session);
            controller.prepare();
            app.setDelegate(Some(ProtocolObject::from_ref(&*controller)));
            app.run();
            controller.stop_timer();
            app.setDelegate(None);
            Ok(())
        })
    }
}

struct ControllerIvars {
    session: RefCell<Session>,
    selected: RefCell<HashSet<Card>>,
    error: RefCell<Option<String>>,
    next_bot_at: Cell<Instant>,
    table: OnceCell<Table>,
    timer: RefCell<Option<Retained<NSTimer>>>,
    rules: RefCell<Option<Retained<NSWindow>>>,
}

// SAFETY: NSObject has no subclassing requirements. All AppKit state and
// callbacks are confined to the main thread; Controller does not implement Drop.
define_class!(
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = ControllerIvars]
    struct Controller;

    unsafe impl NSObjectProtocol for Controller {}

    unsafe impl NSApplicationDelegate for Controller {
        #[unsafe(method(applicationDidFinishLaunching:))]
        fn did_finish_launching(&self, _notification: &NSNotification) {
            self.table().window.makeKeyAndOrderFront(None);
            // Cargo launches an unbundled executable, which needs explicit activation.
            #[allow(deprecated)]
            NSApplication::sharedApplication(self.mtm()).activateIgnoringOtherApps(true);
            self.start_timer();
        }

        #[unsafe(method(applicationShouldTerminateAfterLastWindowClosed:))]
        fn terminate_after_last_window(&self, _app: &NSApplication) -> bool {
            true
        }

        #[unsafe(method(applicationWillTerminate:))]
        fn will_terminate(&self, _notification: &NSNotification) {
            self.stop_timer();
        }
    }

    unsafe impl NSWindowDelegate for Controller {
        #[unsafe(method(windowWillClose:))]
        fn window_will_close(&self, _notification: &NSNotification) {
            self.stop_timer();
            if let Some(rules) = self.ivars().rules.borrow_mut().take() {
                rules.close();
            }
        }
    }

    impl Controller {
        // These selectors are installed only on the corresponding native controls.
        #[unsafe(method(tick:))]
        fn timer_fired(&self, _timer: &NSTimer) {
            self.tick();
        }

        #[unsafe(method(bid:))]
        fn bid(&self, _sender: &NSButton) {
            if let Ok(bid) = u16::try_from(self.table().bid.indexOfSelectedItem()) {
                let minimum = self.ivars().session.borrow().view(0).high_bid + 5;
                self.act(Action::Bid(minimum + bid * 5));
            }
        }

        #[unsafe(method(pass:))]
        fn pass(&self, _sender: &NSButton) {
            self.act(Action::Pass);
        }

        #[unsafe(method(cardClicked:))]
        fn card_clicked(&self, sender: &NSButton) {
            let view = self.ivars().session.borrow().view(0);
            let Some(card) = usize::try_from(sender.tag()).ok()
                .and_then(|index| view.hand.get(index)).copied() else { return };
            if view.turn != Some(0) { return; }
            match view.phase {
                Phase::Discard => {
                    let mut selected = self.ivars().selected.borrow_mut();
                    if !selected.remove(&card) && selected.len() < 5 {
                        selected.insert(card);
                    }
                    drop(selected);
                    self.render();
                }
                Phase::Playing => self.act(Action::Play(card)),
                _ => {}
            }
        }

        #[unsafe(method(discard:))]
        fn discard(&self, _sender: &NSButton) {
            let mut cards: Vec<_> = self.ivars().selected.borrow().iter().copied().collect();
            cards.sort();
            self.act(Action::Discard(cards));
        }

        #[unsafe(method(announce:))]
        fn announce(&self, _sender: &NSButton) {
            let view = self.ivars().session.borrow().view(0);
            let trump = usize::try_from(self.table().trump.indexOfSelectedItem()).ok()
                .and_then(|index| Suit::ALL.get(index)).copied();
            let partner = usize::try_from(self.table().partner.indexOfSelectedItem()).ok()
                .and_then(|index| view.partner_choices.get(index)).copied();
            if let (Some(trump), Some(partner)) = (trump, partner) {
                self.act(Action::Declare { trump, partner });
            }
        }

        #[unsafe(method(nextRound:))]
        fn next_round(&self, _sender: &NSButton) {
            let result = self.ivars().session.borrow_mut().next_round();
            *self.ivars().error.borrow_mut() = result.err().map(|error| error.to_string());
            self.ivars().selected.borrow_mut().clear();
            self.delay(false);
            self.render();
        }

        #[unsafe(method(showRules:))]
        fn show_rules(&self, _sender: &NSObject) {
            if let Some(window) = self.ivars().rules.borrow().as_ref() {
                window.makeKeyAndOrderFront(None);
                return;
            }
            let window = new_window(self.mtm(), "How to play Koor", 660.0, 650.0);
            let (scroll, text) = text_scroll(self.mtm());
            text.setString(&NSString::from_str(concat!(include_str!("../../RULES.md"),
                "\n\nImplementation conventions\n\nPassing removes a player from the auction; an all-pass hand is redealt. A missed bid scores minus the bid for each member of the Leader's team. The opposition scores its captured points. Discarded cards cannot be called as partner. The last trick takes the kitty points.")));
            window.setContentView(Some(&scroll));
            window.center();
            window.makeKeyAndOrderFront(None);
            *self.ivars().rules.borrow_mut() = Some(window);
        }
    }
);

impl Controller {
    fn new(mtm: MainThreadMarker, session: Session) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(ControllerIvars {
            session: RefCell::new(session),
            selected: RefCell::new(HashSet::new()),
            error: RefCell::new(None),
            next_bot_at: Cell::new(Instant::now() + Duration::from_millis(700)),
            table: OnceCell::new(),
            timer: RefCell::new(None),
            rules: RefCell::new(None),
        });
        // SAFETY: NSObject's init has this signature and initializes our superclass.
        unsafe { msg_send![super(this), init] }
    }

    fn prepare(&self) {
        assert!(self.ivars().table.set(Table::new(self)).is_ok());
        self.install_menu();
        self.render();
    }

    fn table(&self) -> &Table {
        self.ivars()
            .table
            .get()
            .expect("table initialized before callbacks")
    }

    fn start_timer(&self) {
        self.stop_timer();
        // SAFETY: tick: accepts an NSTimer, and the timer retains its target.
        let timer = unsafe {
            NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(
                0.1,
                self,
                sel!(tick:),
                None,
                true,
            )
        };
        timer.setTolerance(0.02);
        *self.ivars().timer.borrow_mut() = Some(timer);
    }

    fn stop_timer(&self) {
        // Invalidation releases the timer's target and breaks the retain cycle.
        if let Some(timer) = self.ivars().timer.borrow_mut().take() {
            timer.invalidate();
        }
    }

    fn delay(&self, finished_trick: bool) {
        self.ivars()
            .next_bot_at
            .set(Instant::now() + Duration::from_millis(if finished_trick { 1700 } else { 650 }));
    }

    fn tick(&self) {
        if !self.table().window.isVisible() || Instant::now() < self.ivars().next_bot_at.get() {
            return;
        }
        if self.ivars().session.borrow().game.phase() == Phase::AllPassed {
            let result = self.ivars().session.borrow_mut().next_round();
            *self.ivars().error.borrow_mut() = result.err().map(|error| error.to_string());
            self.ivars().selected.borrow_mut().clear();
            self.delay(false);
            self.render();
            return;
        }
        let before = self.ivars().session.borrow().view(0).tricks_finished;
        let result = self.ivars().session.borrow_mut().step_bot();
        match result {
            Ok(true) => {
                self.delay(self.ivars().session.borrow().view(0).tricks_finished > before);
                self.render();
            }
            Ok(false) => {}
            Err(error) => {
                *self.ivars().error.borrow_mut() = Some(error.to_string());
                self.delay(false);
                self.render();
            }
        }
    }

    fn act(&self, action: Action) {
        let before = self.ivars().session.borrow().view(0).tricks_finished;
        let result = self.ivars().session.borrow_mut().act(0, action);
        *self.ivars().error.borrow_mut() = result.err().map(|error| error.to_string());
        if self.ivars().error.borrow().is_none() {
            self.ivars().selected.borrow_mut().clear();
        }
        self.delay(self.ivars().session.borrow().view(0).tricks_finished > before);
        self.render();
    }

    fn install_menu(&self) {
        let mtm = self.mtm();
        let bar = NSMenu::initWithTitle(NSMenu::alloc(mtm), ns_string!(""));
        let menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), ns_string!("Koor"));
        // SAFETY: These are AppKit's standard responder actions, or our showRules:.
        unsafe {
            let rules = menu.addItemWithTitle_action_keyEquivalent(
                ns_string!("How to play Koor"),
                Some(sel!(showRules:)),
                ns_string!("?"),
            );
            rules.setTarget(Some(self));
            menu.addItem(&NSMenuItem::separatorItem(mtm));
            menu.addItemWithTitle_action_keyEquivalent(
                ns_string!("Hide Koor"),
                Some(sel!(hide:)),
                ns_string!("h"),
            );
            menu.addItemWithTitle_action_keyEquivalent(
                ns_string!("Quit Koor"),
                Some(sel!(terminate:)),
                ns_string!("q"),
            );
        }
        let item = NSMenuItem::new(mtm);
        item.setSubmenu(Some(&menu));
        bar.addItem(&item);
        let window_menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), ns_string!("Window"));
        unsafe {
            window_menu.addItemWithTitle_action_keyEquivalent(
                ns_string!("Minimize"),
                Some(sel!(performMiniaturize:)),
                ns_string!("m"),
            );
            window_menu.addItemWithTitle_action_keyEquivalent(
                ns_string!("Close"),
                Some(sel!(performClose:)),
                ns_string!("w"),
            );
        }
        let item = NSMenuItem::new(mtm);
        item.setTitle(ns_string!("Window"));
        item.setSubmenu(Some(&window_menu));
        bar.addItem(&item);
        let app = NSApplication::sharedApplication(mtm);
        app.setMainMenu(Some(&bar));
        app.setWindowsMenu(Some(&window_menu));
    }

    fn render(&self) {
        let session = self.ivars().session.borrow();
        let view = session.view(0);
        let table = self.table();
        let phase = match view.phase {
            Phase::Bidding => "Bidding",
            Phase::Discard => "Choose five discards",
            Phase::Declare => "Choose trump & partner",
            Phase::Playing => "Play a card",
            Phase::Complete => "Round complete",
            Phase::AllPassed => "Redealing",
        };
        set_text(
            &table.heading,
            &format!("Round {}  ·  {phase}", session.round),
        );
        let contract = if let (Some(trump), Some(partner), Some(leader)) =
            (view.trump, view.partner_card, view.leader)
        {
            let team_scores = view.known_partner.map_or_else(String::new, |partner| {
                let team = view.captured[leader]
                    + if partner == leader {
                        0
                    } else {
                        view.captured[partner]
                    };
                let opposition: u16 = view
                    .captured
                    .iter()
                    .enumerate()
                    .filter(|(player, _)| *player != leader && *player != partner)
                    .map(|(_, points)| *points)
                    .sum();
                format!("\nLeader / partner: {team} pts  ·  Opposition: {opposition} pts")
            });
            format!(
                "{} bid {}  ·  {trump} trump  ·  Partner: {partner}\nKitty {}{team_scores}",
                NAMES[leader],
                view.high_bid,
                if view.kitty_has_points == Some(true) {
                    "has points"
                } else {
                    "has no points"
                }
            )
        } else {
            format!(
                "High bid: {}  ·  180 points in every hand  ·  Play proceeds clockwise",
                view.high_bid
            )
        };
        render_contract(&table.contract, &contract, &view);
        for (player, name) in NAMES.iter().enumerate() {
            let role = if view.leader == Some(player) {
                " · Leader"
            } else if view.revealed_partner == Some(player) {
                " · Partner"
            } else {
                ""
            };
            set_text(
                &table.seats[player],
                &format!(
                    "{}{name}{role}\n{} cards  ·  {} pts",
                    if view.turn == Some(player) {
                        "▶ "
                    } else {
                        ""
                    },
                    view.hand_sizes[player],
                    view.captured[player]
                ),
            );
            let color = if view.turn == Some(player) {
                NSColor::controlAccentColor()
            } else {
                NSColor::labelColor()
            };
            table.seats[player].setTextColor(Some(&color));
        }
        set_text(
            &table.scores,
            &(0..4)
                .map(|player| {
                    format!(
                        "{}    {}\n{} pts this hand",
                        NAMES[player], session.totals[player], view.captured[player],
                    )
                })
                .collect::<Vec<_>>()
                .join("\n\n"),
        );
        let log = format_log(&view.events);
        if table.log.string().to_string() != log {
            table.log.setString(&NSString::from_str(&log));
            table
                .log
                .scrollRangeToVisible(NSRange::new(log.encode_utf16().count(), 0));
        }
        set_text(&table.status, &self.ivars().error.borrow().as_ref()
            .or(session.warning.as_ref()).cloned().unwrap_or_else(|| format!(
                "{} / 10 tricks  ·  {} learning rounds  ·  Koor ranks between 10 and 11 of trump",
                view.tricks_finished, session.learning.rounds,
            )));
        let color = if self.ivars().error.borrow().is_some() || session.warning.is_some() {
            NSColor::systemRedColor()
        } else {
            NSColor::secondaryLabelColor()
        };
        table.status.setTextColor(Some(&color));
        drop(session);

        let previous = view.trick.is_empty() && !view.last_trick.is_empty();
        let displayed = if previous {
            &view.last_trick
        } else {
            &view.trick
        };
        set_text(
            &table.trick_caption,
            &if previous {
                format!("{} took the last trick", NAMES[view.last_winner.unwrap()])
            } else {
                format!("Trick {} of 10", (view.tricks_finished + 1).min(10))
            },
        );
        clear(&table.trick);
        for (player, name) in NAMES.iter().enumerate() {
            let slot = stack(self.mtm(), false, 8.0);
            slot.setAlignment(NSLayoutAttribute::CenterX);
            slot.addArrangedSubview(&label(self.mtm(), name, 12.0, false));
            if let Some((_, card)) = displayed.iter().find(|(seat, _)| *seat == player) {
                let button = card_button(self, *card, view.trump, "");
                // Played cards have no action but retain full suit colors.
                unsafe {
                    button.setAction(None);
                    button.setTarget(None);
                }
                slot.addArrangedSubview(&button);
            } else {
                let empty = label(self.mtm(), "·", 28.0, false);
                empty.setAlignment(NSTextAlignment::Center);
                size(&empty, 78.0, 96.0);
                slot.addArrangedSubview(&empty);
            }
            table.trick.addArrangedSubview(&slot);
        }

        clear(&table.hand);
        let selected = self.ivars().selected.borrow();
        let columns = if view.hand.len() > 8 {
            view.hand.len().div_ceil(2)
        } else {
            8
        };
        for cards in view.hand.chunks(columns) {
            let row = stack(self.mtm(), true, 8.0);
            for card in cards {
                let index = view
                    .hand
                    .iter()
                    .position(|candidate| candidate == card)
                    .unwrap();
                let button = card_button(
                    self,
                    *card,
                    view.trump,
                    card_marker(*card, view.trump, view.partner_card),
                );
                button.setTag(index as isize);
                let discarding = view.phase == Phase::Discard && view.turn == Some(0);
                button.setEnabled(discarding || view.legal_cards.contains(card));
                if discarding {
                    button.setButtonType(NSButtonType::PushOnPushOff);
                    button.setState(if selected.contains(card) {
                        NSControlStateValueOn
                    } else {
                        NSControlStateValueOff
                    });
                }
                row.addArrangedSubview(&button);
            }
            table.hand.addArrangedSubview(&row);
        }
        drop(selected);
        self.render_controls(&view);
    }

    fn render_controls(&self, view: &View) {
        let table = self.table();
        clear(&table.controls);
        if view.phase == Phase::Complete {
            let result = view.result.as_ref().unwrap();
            let partner = if result.partner == result.leader {
                "playing alone".into()
            } else {
                format!("with {}", NAMES[result.partner])
            };
            table.controls.addArrangedSubview(&label(
                self.mtm(),
                &format!(
                    "{} {partner}: {} points against a bid of {}. {}",
                    NAMES[result.leader],
                    result.team_points,
                    result.bid,
                    if result.made_bid {
                        "Bid made!"
                    } else {
                        "Bid missed. Leader's team loses the bid amount."
                    },
                ),
                14.0,
                true,
            ));
            table.controls.addArrangedSubview(&self.button(
                "Deal next round",
                sel!(nextRound:),
                true,
            ));
            return;
        }
        if view.turn != Some(0) {
            table.controls.addArrangedSubview(&label(
                self.mtm(),
                &view
                    .turn
                    .map(|player| format!("{} is thinking…", NAMES[player]))
                    .unwrap_or_else(|| "Everyone passed. Shuffling a new hand…".into()),
                14.0,
                false,
            ));
            return;
        }
        let prompt = match view.phase {
            Phase::Bidding => "Raise the bid or pass. Passing ends your participation in this auction.".into(),
            Phase::Discard => format!("Select five cards to return to the kitty. {} / 5 selected.", self.ivars().selected.borrow().len()),
            Phase::Declare => "Choose trump and a partner card. Calling your own card means secretly playing alone.".into(),
            Phase::Playing => {
                let mut prompt = view.trick.first().map(|(_, card)| format!(
                    "Your turn. Follow {} if you can. Click an enabled card.", card.suit(view.trump.unwrap()),
                )).unwrap_or_else(|| "You lead. Click any card in your hand.".into());
                if view.known_partner == Some(0) && view.revealed_partner.is_none() {
                    prompt.push_str(if view.leader == Some(0) { " You are secretly playing alone." }
                        else { " You hold the partner card!" });
                }
                prompt
            }
            _ => String::new(),
        };
        let prompt = label(self.mtm(), &prompt, 14.0, false);
        table.controls.addArrangedSubview(&prompt);
        // Keep long instructions within the table's width.
        prompt
            .widthAnchor()
            .constraintLessThanOrEqualToAnchor(&table.controls.widthAnchor())
            .setActive(true);
        match view.phase {
            Phase::Bidding => {
                table.bid.removeAllItems();
                for bid in ((view.high_bid + 5)..=180).step_by(5) {
                    table
                        .bid
                        .addItemWithTitle(&NSString::from_str(&bid.to_string()));
                }
                table.bid.selectItemAtIndex(
                    ((view.high_bid + 5).max(80) - view.high_bid - 5) as isize / 5,
                );
                let row = stack(self.mtm(), true, 10.0);
                row.addArrangedSubview(&table.bid);
                row.addArrangedSubview(&self.button("Bid", sel!(bid:), true));
                row.addArrangedSubview(&self.button("Pass", sel!(pass:), false));
                table.controls.addArrangedSubview(&row);
            }
            Phase::Discard => {
                let button = self.button("Discard selected cards", sel!(discard:), true);
                button.setEnabled(self.ivars().selected.borrow().len() == 5);
                table.controls.addArrangedSubview(&button);
            }
            Phase::Declare => {
                table.trump.removeAllItems();
                for suit in Suit::ALL {
                    table
                        .trump
                        .addItemWithTitle(&NSString::from_str(suit.name()));
                }
                table.partner.removeAllItems();
                for card in &view.partner_choices {
                    table.partner.addItemWithTitle(&NSString::from_str(&format!(
                        "{card}{}",
                        if view.hand.contains(card) {
                            " (your card)"
                        } else {
                            ""
                        }
                    )));
                }
                if let Action::Declare { trump, partner } = ai::choose_action(view, Style::Balanced)
                {
                    table.trump.selectItemAtIndex(trump as isize);
                    if let Some(index) = view
                        .partner_choices
                        .iter()
                        .position(|card| *card == partner)
                    {
                        table.partner.selectItemAtIndex(index as isize);
                    }
                }
                let row = stack(self.mtm(), true, 8.0);
                row.addArrangedSubview(&label(self.mtm(), "Trump", 12.0, true));
                row.addArrangedSubview(&table.trump);
                row.addArrangedSubview(&label(self.mtm(), "Partner", 12.0, true));
                row.addArrangedSubview(&table.partner);
                row.addArrangedSubview(&self.button("Announce", sel!(announce:), true));
                table.controls.addArrangedSubview(&row);
            }
            _ => {}
        }
    }

    fn button(
        &self,
        title: &str,
        action: objc2::runtime::Sel,
        primary: bool,
    ) -> Retained<NSButton> {
        // SAFETY: Every action passed here is a Controller selector accepting its sender.
        let button = unsafe {
            NSButton::buttonWithTitle_target_action(
                &NSString::from_str(title),
                Some(self),
                Some(action),
                self.mtm(),
            )
        };
        button.setTranslatesAutoresizingMaskIntoConstraints(false);
        if primary {
            button.setKeyEquivalent(ns_string!("\r"));
        }
        button
    }
}

struct Table {
    window: Retained<NSWindow>,
    heading: Retained<NSTextField>,
    contract: Retained<NSTextField>,
    seats: [Retained<NSTextField>; 4],
    trick_caption: Retained<NSTextField>,
    trick: Retained<NSStackView>,
    hand: Retained<NSStackView>,
    controls: Retained<NSStackView>,
    bid: Retained<NSPopUpButton>,
    trump: Retained<NSPopUpButton>,
    partner: Retained<NSPopUpButton>,
    scores: Retained<NSTextField>,
    log: Retained<NSTextView>,
    status: Retained<NSTextField>,
}

impl Table {
    fn new(controller: &Controller) -> Self {
        let mtm = controller.mtm();
        let window = new_window(mtm, "Koor 😛", 1200.0, 900.0);
        window.setContentMinSize(NSSize::new(1080.0, 850.0));
        window.setDelegate(Some(ProtocolObject::from_ref(controller)));
        let root = stack(mtm, true, 24.0);
        root.setAlignment(NSLayoutAttribute::Top);
        root.setEdgeInsets(NSEdgeInsets {
            top: 22.0,
            left: 22.0,
            bottom: 22.0,
            right: 22.0,
        });
        let content = window.contentView().unwrap();
        content.addSubview(&root);
        pin(&root, &content);
        let main = stack(mtm, false, 10.0);
        main.setAlignment(NSLayoutAttribute::Leading);
        root.addArrangedSubview(&main);
        let sidebar = stack(mtm, false, 16.0);
        sidebar.setAlignment(NSLayoutAttribute::Leading);
        sidebar
            .widthAnchor()
            .constraintEqualToConstant(260.0)
            .setActive(true);
        root.addArrangedSubview(&sidebar);
        // Both columns fill the padded root vertically.
        main.heightAnchor()
            .constraintEqualToAnchor_constant(&root.heightAnchor(), -44.0)
            .setActive(true);
        sidebar
            .heightAnchor()
            .constraintEqualToAnchor_constant(&root.heightAnchor(), -44.0)
            .setActive(true);

        let header = stack(mtm, true, 16.0);
        header.addArrangedSubview(&label(mtm, "KOOR  😛", 26.0, true));
        header.addArrangedSubview(&controller.button("How to play", sel!(showRules:), false));
        main.addArrangedSubview(&header);
        let heading = label(mtm, "", 21.0, true);
        main.addArrangedSubview(&heading);
        let contract = label(mtm, "", 13.0, false);
        contract.setTextColor(Some(&NSColor::secondaryLabelColor()));
        main.addArrangedSubview(&contract);
        contract
            .widthAnchor()
            .constraintEqualToAnchor(&main.widthAnchor())
            .setActive(true);
        contract
            .heightAnchor()
            .constraintGreaterThanOrEqualToConstant(52.0)
            .setActive(true);
        let seats_row = stack(mtm, true, 14.0);
        seats_row.setDistribution(NSStackViewDistribution::FillEqually);
        let seats = std::array::from_fn(|_| {
            let seat = label(mtm, "", 13.0, true);
            seats_row.addArrangedSubview(&seat);
            seat
        });
        main.addArrangedSubview(&seats_row);
        seats_row
            .widthAnchor()
            .constraintEqualToAnchor(&main.widthAnchor())
            .setActive(true);
        let trick_caption = label(mtm, "", 13.0, true);
        main.addArrangedSubview(&trick_caption);
        let trick = stack(mtm, true, 28.0);
        main.addArrangedSubview(&trick);
        let space = NSView::new(mtm);
        space.setTranslatesAutoresizingMaskIntoConstraints(false);
        main.addArrangedSubview(&space);
        space
            .heightAnchor()
            .constraintGreaterThanOrEqualToConstant(4.0)
            .setActive(true);
        let controls = stack(mtm, false, 10.0);
        controls.setAlignment(NSLayoutAttribute::Leading);
        main.addArrangedSubview(&controls);
        controls
            .widthAnchor()
            .constraintEqualToAnchor(&main.widthAnchor())
            .setActive(true);
        controls
            .heightAnchor()
            .constraintGreaterThanOrEqualToConstant(80.0)
            .setActive(true);
        main.addArrangedSubview(&label(
            mtm,
            "YOUR HAND  ·  Select a card to play",
            12.0,
            true,
        ));
        let hand = stack(mtm, false, 8.0);
        hand.setAlignment(NSLayoutAttribute::Leading);
        main.addArrangedSubview(&hand);
        hand.heightAnchor()
            .constraintGreaterThanOrEqualToConstant(200.0)
            .setActive(true);
        let status = label(mtm, "", 11.0, false);
        main.addArrangedSubview(&status);
        status
            .widthAnchor()
            .constraintEqualToAnchor(&main.widthAnchor())
            .setActive(true);
        sidebar.addArrangedSubview(&label(mtm, "SCOREBOARD", 12.0, true));
        let scores = label(mtm, "", 16.0, true);
        sidebar.addArrangedSubview(&scores);
        sidebar.addArrangedSubview(&label(mtm, "AT THE TABLE", 12.0, true));
        let (scroll, log) = text_scroll(mtm);
        sidebar.addArrangedSubview(&scroll);
        scroll
            .widthAnchor()
            .constraintEqualToAnchor(&sidebar.widthAnchor())
            .setActive(true);
        scroll
            .heightAnchor()
            .constraintGreaterThanOrEqualToConstant(160.0)
            .setActive(true);
        let popup = || {
            let popup = NSPopUpButton::initWithFrame_pullsDown(
                NSPopUpButton::alloc(mtm),
                NSRect::ZERO,
                false,
            );
            popup.setTranslatesAutoresizingMaskIntoConstraints(false);
            popup
        };
        window.center();
        Self {
            window,
            heading,
            contract,
            seats,
            trick_caption,
            trick,
            hand,
            controls,
            bid: popup(),
            trump: popup(),
            partner: popup(),
            scores,
            log,
            status,
        }
    }
}

fn new_window(mtm: MainThreadMarker, title: &str, width: f64, height: f64) -> Retained<NSWindow> {
    // SAFETY: Every created window is retained and has release-on-close disabled.
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            NSRect::new(NSPoint::ZERO, NSSize::new(width, height)),
            NSWindowStyleMask::Titled
                | NSWindowStyleMask::Closable
                | NSWindowStyleMask::Miniaturizable
                | NSWindowStyleMask::Resizable,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    unsafe {
        window.setReleasedWhenClosed(false);
    }
    window.setTitle(&NSString::from_str(title));
    window
}

fn stack(mtm: MainThreadMarker, horizontal: bool, spacing: f64) -> Retained<NSStackView> {
    let view = NSStackView::new(mtm);
    view.setOrientation(if horizontal {
        NSUserInterfaceLayoutOrientation::Horizontal
    } else {
        NSUserInterfaceLayoutOrientation::Vertical
    });
    view.setSpacing(spacing);
    view.setTranslatesAutoresizingMaskIntoConstraints(false);
    view.setDistribution(NSStackViewDistribution::Fill);
    view.setDetachesHiddenViews(false);
    view
}

fn label(mtm: MainThreadMarker, text: &str, font_size: f64, bold: bool) -> Retained<NSTextField> {
    let label = NSTextField::wrappingLabelWithString(&NSString::from_str(text), mtm);
    let font = if bold {
        NSFont::boldSystemFontOfSize(font_size)
    } else {
        NSFont::systemFontOfSize(font_size)
    };
    label.setFont(Some(&font));
    label.setTextColor(Some(&NSColor::labelColor()));
    label.setTranslatesAutoresizingMaskIntoConstraints(false);
    label
}

fn set_text(label: &NSTextField, text: &str) {
    label.setStringValue(&NSString::from_str(text));
}

fn clear(stack: &NSStackView) {
    for child in stack.arrangedSubviews() {
        stack.removeArrangedSubview(&child);
        child.removeFromSuperview();
    }
}

fn size(view: &NSView, width: f64, height: f64) {
    view.widthAnchor()
        .constraintEqualToConstant(width)
        .setActive(true);
    view.heightAnchor()
        .constraintEqualToConstant(height)
        .setActive(true);
}

fn pin(child: &NSView, parent: &NSView) {
    child
        .leadingAnchor()
        .constraintEqualToAnchor(&parent.leadingAnchor())
        .setActive(true);
    child
        .trailingAnchor()
        .constraintEqualToAnchor(&parent.trailingAnchor())
        .setActive(true);
    child
        .topAnchor()
        .constraintEqualToAnchor(&parent.topAnchor())
        .setActive(true);
    child
        .bottomAnchor()
        .constraintEqualToAnchor(&parent.bottomAnchor())
        .setActive(true);
}

fn rgb(red: u8, green: u8, blue: u8) -> Retained<NSColor> {
    NSColor::colorWithSRGBRed_green_blue_alpha(
        f64::from(red) / 255.0,
        f64::from(green) / 255.0,
        f64::from(blue) / 255.0,
        1.0,
    )
}

fn suit_color(suit: Option<Suit>, dark: bool) -> Retained<NSColor> {
    match (suit, dark) {
        (Some(Suit::Purple), true) => rgb(196, 156, 245),
        (Some(Suit::Pink), true) => rgb(255, 145, 194),
        (Some(Suit::Orange), true) => rgb(255, 179, 97),
        (Some(Suit::Brown), true) => rgb(219, 176, 135),
        (None, true) => rgb(158, 217, 176),
        (Some(Suit::Purple), false) => rgb(113, 54, 162),
        (Some(Suit::Pink), false) => rgb(174, 47, 105),
        (Some(Suit::Orange), false) => rgb(165, 72, 9),
        (Some(Suit::Brown), false) => rgb(112, 64, 34),
        (None, false) => rgb(37, 76, 69),
    }
}

fn span(text: &str, part: &str) -> NSRange {
    let offset = text.find(part).expect("styled text contains its span");
    NSRange::new(
        text[..offset].encode_utf16().count(),
        part.encode_utf16().count(),
    )
}

fn attributed_text(
    text: &str,
    font: &NSFont,
    color: &NSColor,
) -> Retained<NSMutableAttributedString> {
    // SAFETY: The attribute keys contain their documented NSFont/NSColor types.
    unsafe {
        let attributes = NSDictionary::from_slices(
            &[NSFontAttributeName, NSForegroundColorAttributeName],
            &[font as &AnyObject, color as &AnyObject],
        );
        NSMutableAttributedString::initWithString_attributes(
            NSMutableAttributedString::alloc(),
            &NSString::from_str(text),
            Some(&attributes),
        )
    }
}

fn render_contract(label: &NSTextField, text: &str, view: &View) {
    let attributed = attributed_text(
        text,
        &NSFont::systemFontOfSize(13.0),
        &NSColor::secondaryLabelColor(),
    );
    if let Some(trump) = view.trump {
        let trump_span = span(text, &format!("{trump} trump"));
        let kitty_span = span(
            text,
            if view.kitty_has_points == Some(true) {
                "Kitty has points"
            } else {
                "Kitty has no points"
            },
        );
        let kitty_color = if view.kitty_has_points == Some(true) {
            rgb(36, 102, 60)
        } else {
            rgb(146, 59, 59)
        };
        // SAFETY: Every span is a UTF-16 range within this string; color and
        // font attribute values have the types required by AppKit.
        unsafe {
            for range in [trump_span, kitty_span] {
                attributed.addAttribute_value_range(
                    NSFontAttributeName,
                    &NSFont::boldSystemFontOfSize(13.0),
                    range,
                );
            }
            attributed.addAttribute_value_range(
                NSForegroundColorAttributeName,
                &suit_color(Some(trump), false),
                trump_span,
            );
            attributed.addAttribute_value_range(
                NSBackgroundColorAttributeName,
                &rgb(250, 245, 234),
                trump_span,
            );
            attributed.addAttribute_value_range(
                NSForegroundColorAttributeName,
                &NSColor::whiteColor(),
                kitty_span,
            );
            attributed.addAttribute_value_range(
                NSBackgroundColorAttributeName,
                &kitty_color,
                kitty_span,
            );
        }
    }
    label.setAttributedStringValue(&attributed);
}

struct CardButtonIvars {
    title: Retained<NSAttributedString>,
}

// SAFETY: NSButton permits custom drawing. This subclass preserves native
// control state, target/actions and accessibility, and draws only on the main
// thread. It has no Drop implementation.
define_class!(
    #[unsafe(super = NSButton)]
    #[thread_kind = MainThreadOnly]
    #[ivars = CardButtonIvars]
    struct NativeCardButton;

    unsafe impl NSObjectProtocol for NativeCardButton {}

    impl NativeCardButton {
        #[unsafe(method(wantsUpdateLayer))]
        fn wants_update_layer(&self) -> bool { false }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            let bounds = self.bounds();
            if let Some(cell) = self.cell().and_then(|cell| cell.downcast::<NSButtonCell>().ok()) {
                cell.drawBezelWithFrame_inView(bounds, self);
            }
            // NSButton's normal title drawing replaces suit colors with gray
            // when disabled. Draw the original attributed title over the native
            // bezel so disabling play does not remove card information.
            let height = self.ivars().title.size().height;
            self.ivars().title.drawInRect(NSRect::new(
                NSPoint::new(4.0, (bounds.size.height - height) / 2.0),
                NSSize::new(bounds.size.width - 8.0, height),
            ));
        }
    }
);

fn card_button(
    controller: &Controller,
    card: Card,
    trump: Option<Suit>,
    marker: &str,
) -> Retained<NSButton> {
    let suit = match card {
        Card::Number(suit, _) => Some(suit),
        Card::Koor => trump,
    };
    let face = card.face();
    let value = format!("{} pts", card.points());
    let mut title = format!("{face}\n{}\n{value}", suit.map_or("KOOR", Suit::name));
    if !marker.is_empty() {
        title.push('\n');
        title.push_str(marker);
    }
    // SAFETY: These are immutable framework appearance-name constants.
    let dark = unsafe {
        controller
            .table()
            .window
            .effectiveAppearance()
            .bestMatchFromAppearancesWithNames(&NSArray::from_slice(&[
                NSAppearanceNameAqua,
                NSAppearanceNameDarkAqua,
            ]))
            .is_some_and(|name| *name == *NSAppearanceNameDarkAqua)
    };
    let attributed = attributed_text(
        &title,
        &NSFont::boldSystemFontOfSize(12.0),
        &suit_color(suit, dark),
    );
    let paragraph = NSMutableParagraphStyle::new();
    paragraph.setAlignment(NSTextAlignment::Center);
    // SAFETY: Attribute values have their documented types and spans account
    // for the Koor emoji's two UTF-16 code units.
    unsafe {
        attributed.addAttribute_value_range(
            NSParagraphStyleAttributeName,
            &paragraph,
            NSRange::new(0, attributed.length()),
        );
        attributed.addAttribute_value_range(
            NSFontAttributeName,
            &NSFont::boldSystemFontOfSize(24.0),
            span(&title, &face),
        );
        attributed.addAttribute_value_range(
            NSFontAttributeName,
            &NSFont::systemFontOfSize(11.0),
            span(&title, &value),
        );
        attributed.addAttribute_value_range(
            NSForegroundColorAttributeName,
            &NSColor::secondaryLabelColor(),
            span(&title, &value),
        );
        if !marker.is_empty() {
            let color = if dark {
                rgb(117, 173, 255)
            } else {
                rgb(23, 100, 209)
            };
            attributed.addAttribute_value_range(
                NSFontAttributeName,
                &NSFont::boldSystemFontOfSize(18.0),
                span(&title, marker),
            );
            attributed.addAttribute_value_range(
                NSForegroundColorAttributeName,
                &color,
                span(&title, marker),
            );
        }
    }
    let this = NativeCardButton::alloc(controller.mtm()).set_ivars(CardButtonIvars {
        title: attributed.clone().into_super(),
    });
    // SAFETY: NSButton's designated initializer has this signature. The target
    // remains retained by the frontend and cardClicked: accepts an NSButton.
    let button: Retained<NativeCardButton> =
        unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
    unsafe {
        button.setTarget(Some(controller));
        button.setAction(Some(sel!(cardClicked:)));
    }
    button.setTranslatesAutoresizingMaskIntoConstraints(false);
    button.setBezelStyle(NSBezelStyle::FlexiblePush);
    button.setBordered(true);
    button.setAttributedTitle(&attributed);
    button.setToolTip(Some(&NSString::from_str(&format!(
        "{card} · {} points",
        card.points()
    ))));
    size(&button, 78.0, 96.0);
    button.into_super()
}

fn text_scroll(mtm: MainThreadMarker) -> (Retained<NSScrollView>, Retained<NSTextView>) {
    let scroll = NSScrollView::new(mtm);
    scroll.setTranslatesAutoresizingMaskIntoConstraints(false);
    scroll.setHasVerticalScroller(true);
    scroll.setAutohidesScrollers(true);
    scroll.setBorderType(NSBorderType::BezelBorder);
    let text = NSTextView::initWithFrame(
        NSTextView::alloc(mtm),
        NSRect::new(NSPoint::ZERO, NSSize::new(260.0, 400.0)),
    );
    text.setEditable(false);
    text.setSelectable(true);
    text.setRichText(false);
    text.setFont(Some(&NSFont::systemFontOfSize(13.0)));
    text.setTextColor(Some(&NSColor::labelColor()));
    text.setBackgroundColor(&NSColor::textBackgroundColor());
    text.setTextContainerInset(NSSize::new(12.0, 12.0));
    text.setVerticallyResizable(true);
    text.setHorizontallyResizable(false);
    text.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable);
    text.setMinSize(NSSize::ZERO);
    text.setMaxSize(NSSize::new(f64::MAX, f64::MAX));
    // SAFETY: The container is accessed on its text view's main thread.
    if let Some(container) = unsafe { text.textContainer() } {
        container.setWidthTracksTextView(true);
        container.setContainerSize(NSSize::new(260.0, f64::MAX));
    }
    scroll.setDocumentView(Some(&text));
    (scroll, text)
}
