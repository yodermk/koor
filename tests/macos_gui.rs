// AppKit requires the process main thread, so this integration test uses its
// own harness and opens windows only when explicitly run with --ignored.
#[cfg(target_os = "macos")]
use koor::{ai, cards, game, session, ui::Frontend};

#[cfg(target_os = "macos")]
#[path = "../src/ui/presentation.rs"]
mod presentation;

#[cfg(target_os = "macos")]
#[allow(dead_code)]
mod native {
    include!("../src/ui/macos.rs");
    use objc2_foundation::{NSDate, NSDefaultRunLoopMode, NSRunLoop};

    fn pump_events() {
        let until = Instant::now() + Duration::from_millis(40);
        while Instant::now() < until {
            let deadline = NSDate::dateWithTimeIntervalSinceNow(0.01);
            unsafe {
                NSRunLoop::currentRunLoop().runMode_beforeDate(NSDefaultRunLoopMode, &deadline);
            }
        }
    }

    fn descendants(view: &NSView) -> Vec<Retained<NSView>> {
        let mut result = Vec::new();
        for child in view.subviews() {
            result.extend(descendants(&child));
            result.push(child);
        }
        result
    }

    fn control(controller: &Controller, title: &str) -> Retained<NSButton> {
        descendants(&controller.table().controls)
            .into_iter()
            .filter_map(|view| view.downcast::<NSButton>().ok())
            .find(|button| button.title().to_string() == title)
            .expect("control exists")
    }

    fn click(controller: &Controller, title: &str) {
        let button = control(controller, title);
        assert!(button.isEnabled(), "{title} must be enabled");
        // SAFETY: The target and selector were installed by the frontend.
        unsafe {
            button.performClick(None);
        }
    }

    fn hand_button(controller: &Controller, card: Card) -> Retained<NSButton> {
        let view = controller.ivars().session.borrow().view(0);
        let index = view
            .hand
            .iter()
            .position(|candidate| *candidate == card)
            .unwrap();
        descendants(&controller.table().hand)
            .into_iter()
            .filter_map(|view| view.downcast::<NSButton>().ok())
            .find(|button| button.tag() == index as isize)
            .expect("hand card exists")
    }

    fn click_card(controller: &Controller, card: Card) {
        let button = hand_button(controller, card);
        assert!(button.isEnabled());
        unsafe {
            button.performClick(None);
        }
    }

    fn assert_rendered_color(view: &NSView, expected: (u8, u8, u8)) {
        view.setNeedsDisplay(true);
        let bitmap = view
            .bitmapImageRepForCachingDisplayInRect(view.bounds())
            .unwrap();
        view.cacheDisplayInRect_toBitmapImageRep(view.bounds(), &bitmap);
        let expected = [expected.0, expected.1, expected.2].map(|value| f64::from(value) / 255.0);
        let expected_spread = expected.iter().copied().fold(0.0_f64, f64::max)
            - expected.iter().copied().fold(1.0_f64, f64::min);
        let mut matching = 0;
        let mut closest = (f64::MAX, [0.0; 4]);
        for y in 0..bitmap.pixelsHigh() {
            for x in 0..bitmap.pixelsWide() {
                let color = bitmap
                    .colorAtX_y(x, y)
                    .unwrap()
                    .colorUsingColorSpace(&NSColorSpace::sRGBColorSpace())
                    .unwrap();
                let actual = [
                    color.redComponent(),
                    color.greenComponent(),
                    color.blueComponent(),
                ];
                // Allow antialiasing and display-profile conversion while rejecting
                // grayscale text. The drawing attributes use the exact sRGB colors.
                let alpha = color.alphaComponent();
                let spread = actual.iter().copied().fold(0.0_f64, f64::max)
                    - actual.iter().copied().fold(1.0_f64, f64::min);
                if alpha > 0.4 {
                    let distance = actual
                        .iter()
                        .zip(expected)
                        .map(|(a, b)| (a - b).abs())
                        .fold(0.0_f64, f64::max);
                    if distance < closest.0 {
                        closest = (distance, [actual[0], actual[1], actual[2], alpha]);
                    }
                }
                if alpha > 0.4
                    && spread > expected_spread * 0.6
                    && (actual
                        .iter()
                        .zip(expected)
                        .all(|(a, b)| (a - b).abs() < 0.15)
                        || actual
                            .iter()
                            .zip(expected)
                            .all(|(a, b)| (a / alpha - b).abs() < 0.15))
                {
                    matching += 1;
                    if matching >= 8 {
                        return;
                    }
                }
            }
        }
        if let Some(directory) = std::env::var_os("KOOR_SCREENSHOT_DIR") {
            let data = unsafe {
                bitmap.representationUsingType_properties(
                    NSBitmapImageFileType::PNG,
                    &NSDictionary::new(),
                )
            }
            .unwrap();
            std::fs::write(
                std::path::PathBuf::from(directory).join("macos-color-failure.png"),
                data.to_vec(),
            )
            .unwrap();
        }
        panic!(
            "rendered view lost color {expected:?}; closest {closest:?}; frame {:?}",
            view.frame()
        );
    }

    fn assert_card_rendering(controller: &Controller) {
        let window = new_window(controller.mtm(), "Card colors", 160.0, 140.0);
        let content = window.contentView().unwrap();
        window.makeKeyAndOrderFront(None);
        for (suit, expected) in [
            (Suit::Purple, (113, 54, 162)),
            (Suit::Pink, (174, 47, 105)),
            (Suit::Orange, (165, 72, 9)),
            (Suit::Brown, (112, 64, 34)),
        ] {
            for card in [Card::Number(suit, 14), Card::Koor] {
                let button = card_button(controller, card, Some(suit), "* P");
                content.addSubview(&button);
                button
                    .leadingAnchor()
                    .constraintEqualToAnchor_constant(&content.leadingAnchor(), 20.0)
                    .setActive(true);
                button
                    .topAnchor()
                    .constraintEqualToAnchor_constant(&content.topAnchor(), 20.0)
                    .setActive(true);
                button.setButtonType(NSButtonType::PushOnPushOff);
                for enabled in [true, false] {
                    button.setEnabled(enabled);
                    for state in [NSControlStateValueOff, NSControlStateValueOn] {
                        button.setState(state);
                        pump_events();
                        content.layoutSubtreeIfNeeded();
                        assert_rendered_color(&button, expected);
                        assert_rendered_color(&button, (23, 100, 209));
                    }
                }
                button.removeFromSuperview();
            }
        }
        window.close();
    }

    fn screenshot(controller: &Controller, name: &str) {
        pump_events();
        let content = controller.table().window.contentView().unwrap();
        content.layoutSubtreeIfNeeded();
        assert_eq!(
            controller.table().log.string().to_string(),
            format_log(&controller.ivars().session.borrow().view(0).events)
        );
        assert!(controller.table().hand.frame().size.height >= 190.0);
        assert!(controller.table().controls.frame().size.width >= 680.0);
        assert!(controller.table().status.frame().size.width >= 680.0);
        if let Some(directory) = std::env::var_os("KOOR_SCREENSHOT_DIR") {
            let bitmap = content
                .bitmapImageRepForCachingDisplayInRect(content.bounds())
                .unwrap();
            content.cacheDisplayInRect_toBitmapImageRep(content.bounds(), &bitmap);
            // The content view is transparent; the window normally paints its
            // background. Include that background in the saved preview.
            let context = NSGraphicsContext::graphicsContextWithBitmapImageRep(&bitmap).unwrap();
            NSGraphicsContext::saveGraphicsState_class();
            NSGraphicsContext::setCurrentContext(Some(&context));
            NSColor::windowBackgroundColor().setFill();
            NSRectFillUsingOperation(content.bounds(), NSCompositingOperation::DestinationOver);
            NSGraphicsContext::restoreGraphicsState_class();
            // SAFETY: An empty properties dictionary is valid for PNG encoding.
            let data = unsafe {
                bitmap.representationUsingType_properties(
                    NSBitmapImageFileType::PNG,
                    &NSDictionary::new(),
                )
            }
            .unwrap();
            std::fs::write(
                std::path::PathBuf::from(directory).join(format!("macos-{name}.png")),
                data.to_vec(),
            )
            .unwrap();
        }
    }

    pub(super) fn run() {
        let mtm = MainThreadMarker::new().expect("native test runs on the process main thread");
        autoreleasepool(|_| {
            let app = NSApplication::sharedApplication(mtm);
            app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
            app.finishLaunching();
            let controller = Controller::new(mtm, Session::new(42, Some(0), None).unwrap());
            controller.prepare();
            // Keep the color regression checks independent of desktop appearance.
            let appearance =
                unsafe { NSAppearance::appearanceNamed(NSAppearanceNameAqua) }.unwrap();
            controller.table().window.setAppearance(Some(&appearance));
            controller.render();
            controller.table().window.makeKeyAndOrderFront(None);
            assert_card_rendering(&controller);
            controller.start_timer();
            let mut discarded = false;
            let mut declared = false;
            let mut plays = 0;
            for _ in 0..200 {
                let view = controller.ivars().session.borrow().view(0);
                if view.phase == Phase::Complete {
                    break;
                }
                if view.turn != Some(0) {
                    controller.ivars().next_bot_at.set(Instant::now());
                    controller.ivars().timer.borrow().as_ref().unwrap().fire();
                    continue;
                }
                match view.phase {
                    Phase::Bidding => {
                        screenshot(&controller, "bidding");
                        controller
                            .table()
                            .bid
                            .selectItemWithTitle(ns_string!("180"));
                        click(&controller, "Bid");
                    }
                    Phase::Discard => {
                        assert!(!control(&controller, "Discard selected cards").isEnabled());
                        let discards = ai::choose_discards(&view.hand);
                        for card in &discards {
                            click_card(&controller, *card);
                        }
                        assert_eq!(controller.ivars().selected.borrow().len(), 5);
                        let sixth = *view
                            .hand
                            .iter()
                            .find(|card| !discards.contains(card))
                            .unwrap();
                        click_card(&controller, sixth);
                        assert_eq!(controller.ivars().selected.borrow().len(), 5);
                        assert!(!controller.ivars().selected.borrow().contains(&sixth));
                        click_card(&controller, discards[0]);
                        assert!(!control(&controller, "Discard selected cards").isEnabled());
                        click_card(&controller, discards[0]);
                        // Check that resizing preserves both rows of the 15-card hand.
                        controller
                            .table()
                            .window
                            .setContentSize(NSSize::new(1080.0, 850.0));
                        screenshot(&controller, "discard");
                        click(&controller, "Discard selected cards");
                        discarded = true;
                    }
                    Phase::Declare => {
                        let Action::Declare { trump, .. } =
                            ai::choose_action(&view, Style::Balanced)
                        else {
                            unreachable!()
                        };
                        // Call a card we hold so the real hand renders both
                        // trump and partner markers, including while disabled.
                        let partner = *view
                            .hand
                            .iter()
                            .filter(|card| card.suit(trump) == trump)
                            .max_by_key(|card| card.strength())
                            .unwrap();
                        controller.table().trump.selectItemAtIndex(trump as isize);
                        controller.table().partner.selectItemAtIndex(
                            view.partner_choices
                                .iter()
                                .position(|card| *card == partner)
                                .unwrap() as isize,
                        );
                        screenshot(&controller, "declare");
                        click(&controller, "Announce");
                        assert!(
                            hand_button(&controller, partner)
                                .attributedTitle()
                                .string()
                                .to_string()
                                .ends_with("* P")
                        );
                        screenshot(&controller, "playing");
                        let view = controller.ivars().session.borrow().view(0);
                        assert_rendered_color(&controller.table().contract, (174, 47, 105));
                        assert_rendered_color(
                            &controller.table().contract,
                            if view.kitty_has_points == Some(true) {
                                (36, 102, 60)
                            } else {
                                (146, 59, 59)
                            },
                        );
                        let mut empty_kitty = view.clone();
                        empty_kitty.kitty_has_points = Some(false);
                        let text = controller
                            .table()
                            .contract
                            .stringValue()
                            .to_string()
                            .replace("Kitty has points", "Kitty has no points");
                        render_contract(&controller.table().contract, &text, &empty_kitty);
                        assert_rendered_color(&controller.table().contract, (146, 59, 59));
                        controller.render();
                        declared = true;
                    }
                    Phase::Playing => {
                        for card in &view.hand {
                            let marker = card_marker(*card, view.trump, view.partner_card);
                            let title = hand_button(&controller, *card)
                                .attributedTitle()
                                .string()
                                .to_string();
                            if !marker.is_empty() {
                                assert!(title.ends_with(marker));
                            }
                            assert_eq!(
                                hand_button(&controller, *card).isEnabled(),
                                view.legal_cards.contains(card)
                            );
                        }
                        let Action::Play(card) = ai::choose_action(&view, Style::Balanced) else {
                            unreachable!()
                        };
                        click_card(&controller, card);
                        plays += 1;
                        if plays == 1 {
                            screenshot(&controller, "disabled-hand");
                        }
                    }
                    _ => panic!("unexpected phase"),
                }
                assert!(controller.ivars().error.borrow().is_none());
            }
            assert!(discarded && declared);
            assert_eq!(plays, 10);
            assert_eq!(
                controller.ivars().session.borrow().game.phase(),
                Phase::Complete
            );
            let scores = controller
                .ivars()
                .session
                .borrow()
                .view(0)
                .result
                .unwrap()
                .scores
                .map(i64::from);
            assert_eq!(controller.ivars().session.borrow().totals, scores);
            screenshot(&controller, "complete");
            click(&controller, "Deal next round");
            assert_eq!(controller.ivars().session.borrow().round, 2);
            assert_eq!(controller.ivars().session.borrow().totals, scores);
            // Real Pass control and timer-driven all-pass redeal.
            while controller.ivars().session.borrow().game.phase() == Phase::Bidding {
                let player = controller.ivars().session.borrow().game.turn().unwrap();
                if player == 0 {
                    click(&controller, "Pass");
                } else {
                    controller
                        .ivars()
                        .session
                        .borrow_mut()
                        .act(player, Action::Pass)
                        .unwrap();
                    controller.render();
                }
            }
            assert_eq!(
                controller.ivars().session.borrow().game.phase(),
                Phase::AllPassed
            );
            controller.ivars().next_bot_at.set(Instant::now());
            controller.ivars().timer.borrow().as_ref().unwrap().fire();
            assert_eq!(
                controller.ivars().session.borrow().game.phase(),
                Phase::Bidding
            );
            assert_eq!(controller.ivars().session.borrow().round, 2);
            let rules = descendants(&controller.table().window.contentView().unwrap())
                .into_iter()
                .filter_map(|view| view.downcast::<NSButton>().ok())
                .find(|button| button.title().to_string() == "How to play")
                .unwrap();
            unsafe {
                rules.performClick(None);
            }
            assert!(
                controller
                    .ivars()
                    .rules
                    .borrow()
                    .as_ref()
                    .unwrap()
                    .isVisible()
            );
            let timer = controller.ivars().timer.borrow().as_ref().unwrap().clone();
            controller.table().window.close();
            assert!(!timer.isValid());
            assert!(controller.ivars().rules.borrow().is_none());
            app.setMainMenu(None);
        });
        println!(
            "AppKit controls passed: log grouping, status colors, colored disabled/selected cards, trump/partner markers, complete gameplay, next round, redeal, rules, resize, and timer cleanup."
        );
    }
}

fn main() {
    #[cfg(target_os = "macos")]
    if std::env::args().any(|argument| argument == "--ignored") {
        native::run();
        return;
    }
    println!(
        "macos_gui skipped: run on a macOS desktop with cargo test --test macos_gui -- --ignored"
    );
}
