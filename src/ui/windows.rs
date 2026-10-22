//! Native Win32 controls. The message thread owns every HWND and the boxed Table.
//! The user-data pointer is installed after construction and cleared before drop.
//! State mutations finish before any operation that can dispatch window messages.
#![allow(unsafe_op_in_unsafe_fn)]

use super::{
    Frontend,
    presentation::{card_marker, format_log},
};
use crate::{
    cards::{Card, Suit},
    game::{Action, NAMES, Phase},
    session::Session,
};
use std::{
    cell::{Cell, RefCell},
    collections::HashSet,
    time::{Duration, Instant},
};
use windows::{
    Win32::{
        Foundation::*,
        Graphics::Gdi::*,
        System::LibraryLoader::{GetModuleHandleW, LoadLibraryW},
        UI::{
            Controls::RichEdit::*,
            Controls::*,
            HiDpi::*,
            Input::KeyboardAndMouse::{EnableWindow, GetFocus, SetFocus},
            WindowsAndMessaging::*,
        },
    },
    core::{PCWSTR, w},
};

const BID: usize = 10;
const PASS: usize = 11;
const DISCARD: usize = 12;
const DECLARE: usize = 13;
const NEXT: usize = 14;
const RULES: usize = 15;
const BID_CHOICE: usize = 20;
const TRUMP: usize = 21;
const PARTNER: usize = 22;
const RULES_TEXT: i32 = 30;
const HAND: usize = 100;
const TRICK: usize = 200;
const TIMER: usize = 1;

pub struct WindowsFrontend;
impl Frontend for WindowsFrontend {
    fn run(self, session: Session) -> Result<(), String> {
        // Native resources stay on this thread and are released by Table.
        unsafe { run(session).map_err(|error| format!("Windows UI: {error}")) }
    }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

unsafe fn send(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    SendMessageW(hwnd, message, Some(wparam), Some(lparam))
}

struct State {
    session: Session,
    selected: HashSet<Card>,
    error: Option<String>,
    next_bot: Instant,
}

impl State {
    fn new(session: Session) -> Self {
        Self {
            session,
            selected: HashSet::new(),
            error: None,
            next_bot: Instant::now() + Duration::from_millis(700),
        }
    }

    fn act(&mut self, action: Action) {
        let before = self.session.view(0).tricks_finished;
        self.error = self.session.act(0, action).err().map(|e| e.to_string());
        if self.error.is_none() {
            self.selected.clear();
        }
        self.pause(before);
    }

    fn pause(&mut self, before: usize) {
        self.next_bot = Instant::now()
            + Duration::from_millis(if self.session.view(0).tricks_finished > before {
                1700
            } else {
                650
            });
    }

    fn next_round(&mut self) {
        self.error = self.session.next_round().err().map(|e| e.to_string());
        self.selected.clear();
        self.next_bot = Instant::now() + Duration::from_millis(700);
    }

    fn tick(&mut self) -> bool {
        if Instant::now() < self.next_bot || self.error.is_some() {
            return false;
        }
        if self.session.view(0).phase == Phase::AllPassed {
            self.next_round();
            return true;
        }
        let before = self.session.view(0).tricks_finished;
        match self.session.step_bot() {
            Ok(false) => false,
            Ok(true) => {
                self.pause(before);
                true
            }
            Err(error) => {
                self.error = Some(error.to_string());
                true
            }
        }
    }
}

struct Table {
    hwnd: HWND,
    state: RefCell<State>,
    controls: Vec<(usize, HWND)>,
    heading: HWND,
    contract: HWND,
    seats: HWND,
    caption: HWND,
    prompt: HWND,
    scores: HWND,
    log: HWND,
    status: HWND,
    font: Cell<HFONT>,
    card_font: Cell<HFONT>,
    rules: RefCell<Option<HWND>>,
    dpi: Cell<u32>,
    _rich_edit: RichEditLibrary,
}

// Keep the control's window procedure loaded until all child windows are destroyed.
struct RichEditLibrary(HMODULE);
impl Drop for RichEditLibrary {
    fn drop(&mut self) {
        unsafe {
            let _ = FreeLibrary(self.0);
        }
    }
}

unsafe fn run(session: Session) -> windows::core::Result<()> {
    // Failure means the host already selected a DPI awareness context.
    let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    let instance = GetModuleHandleW(None)?;
    let class = WNDCLASSW {
        lpfnWndProc: Some(window_proc),
        hInstance: instance.into(),
        lpszClassName: w!("Koor.Table"),
        hCursor: LoadCursorW(None, IDC_ARROW)?,
        hbrBackground: GetSysColorBrush(COLOR_WINDOW),
        ..Default::default()
    };
    if RegisterClassW(&class) == 0 {
        return Err(windows::core::Error::from_thread());
    }
    let result = (|| {
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class.lpszClassName,
            w!("Koor 😛"),
            WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            1180,
            880,
            None,
            None,
            Some(instance.into()),
            None,
        )?;
        let table = Box::new(Table::new(hwnd, session)?);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, (&*table as *const Table) as isize);
        let dpi = table.dpi.get() as i32;
        let _ = SetWindowPos(
            hwnd,
            None,
            0,
            0,
            1180 * dpi / 96,
            880 * dpi / 96,
            SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
        );
        table.layout();
        table.render();
        if SetTimer(Some(hwnd), TIMER, 100, None) == 0 {
            return Err(windows::core::Error::from_thread());
        }
        let _ = ShowWindow(hwnd, SW_SHOW);
        let mut message = MSG::default();
        loop {
            let status = GetMessageW(&mut message, None, 0, 0).0;
            if status == -1 {
                break Err(windows::core::Error::from_thread());
            }
            if status == 0 {
                break Ok(());
            }
            let rules = *table.rules.borrow();
            if rules.is_some_and(|rules| IsDialogMessageW(rules, &message).as_bool()) {
                continue;
            }
            if IsDialogMessageW(hwnd, &message).as_bool() {
                continue;
            }
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    })();
    let _ = UnregisterClassW(class.lpszClassName, Some(instance.into()));
    result
}

impl Table {
    unsafe fn new(hwnd: HWND, session: Session) -> windows::core::Result<Self> {
        let rich_edit = match LoadLibraryW(w!("Msftedit.dll")) {
            Ok(module) => RichEditLibrary(module),
            Err(error) => {
                let _ = DestroyWindow(hwnd);
                return Err(error);
            }
        };
        let mut table = Self {
            hwnd,
            state: RefCell::new(State::new(session)),
            controls: vec![],
            heading: HWND::default(),
            contract: HWND::default(),
            seats: HWND::default(),
            caption: HWND::default(),
            prompt: HWND::default(),
            scores: HWND::default(),
            log: HWND::default(),
            status: HWND::default(),
            font: Cell::new(HFONT::default()),
            card_font: Cell::new(HFONT::default()),
            rules: RefCell::new(None),
            dpi: Cell::new(GetDpiForWindow(hwnd)),
            _rich_edit: rich_edit,
        };
        table.update_fonts();
        table.heading = table.child(w!("STATIC"), "", 0, WINDOW_STYLE::default())?;
        table.contract = table.child(
            w!("RICHEDIT50W"),
            "",
            0,
            WINDOW_STYLE((ES_MULTILINE | ES_READONLY) as u32),
        )?;
        send(table.contract, EM_SETBKGNDCOLOR, WPARAM(1), LPARAM(0));
        table.seats = table.child(w!("STATIC"), "", 0, WINDOW_STYLE::default())?;
        table.caption = table.child(w!("STATIC"), "", 0, WINDOW_STYLE::default())?;
        table.prompt = table.child(w!("STATIC"), "", 0, WINDOW_STYLE::default())?;
        table.scores = table.child(w!("STATIC"), "", 0, WINDOW_STYLE::default())?;
        table.status = table.child(w!("STATIC"), "", 0, WINDOW_STYLE::default())?;
        table.log = table.child(
            w!("EDIT"),
            "",
            0,
            WS_BORDER
                | WS_VSCROLL
                | WS_TABSTOP
                | WINDOW_STYLE((ES_MULTILINE | ES_READONLY | ES_AUTOVSCROLL) as u32),
        )?;
        send(table.log, EM_SETLIMITTEXT, WPARAM(1_000_000), LPARAM(0));
        for (id, title) in [
            (BID, "&Bid"),
            (PASS, "&Pass"),
            (DISCARD, "&Discard selected cards"),
            (DECLARE, "&Announce"),
            (NEXT, "Deal &next round"),
            (RULES, "&How to play"),
        ] {
            let control = table.child(w!("BUTTON"), title, id, WS_TABSTOP)?;
            table.controls.push((id, control));
        }
        for (id, name) in [
            (BID_CHOICE, "Bid amount"),
            (TRUMP, "Trump suit"),
            (PARTNER, "Partner card"),
        ] {
            let control = table.child(
                w!("COMBOBOX"),
                name,
                id,
                WS_TABSTOP | WS_VSCROLL | WINDOW_STYLE(CBS_DROPDOWNLIST as u32),
            )?;
            table.controls.push((id, control));
        }
        for id in (HAND..HAND + 15).chain(TRICK..TRICK + 4) {
            let control = table.child(
                w!("BUTTON"),
                "",
                id,
                WS_TABSTOP | WINDOW_STYLE(BS_OWNERDRAW as u32),
            )?;
            table.controls.push((id, control));
        }
        Ok(table)
    }

    unsafe fn update_fonts(&self) {
        let font = |size: i32, weight: i32| {
            CreateFontW(
                -(size * self.dpi.get() as i32 / 96),
                0,
                0,
                0,
                weight,
                0,
                0,
                0,
                DEFAULT_CHARSET,
                OUT_DEFAULT_PRECIS,
                CLIP_DEFAULT_PRECIS,
                CLEARTYPE_QUALITY,
                DEFAULT_PITCH.0 as u32,
                w!("Segoe UI"),
            )
        };
        let old = self.font.replace(font(16, 400));
        let old_card = self.card_font.replace(font(18, 600));
        for hwnd in [
            self.heading,
            self.contract,
            self.seats,
            self.caption,
            self.prompt,
            self.scores,
            self.log,
            self.status,
        ]
        .into_iter()
        .chain(self.controls.iter().map(|(_, hwnd)| *hwnd))
        {
            if !hwnd.is_invalid() {
                send(
                    hwnd,
                    WM_SETFONT,
                    WPARAM(self.font.get().0 as usize),
                    LPARAM(1),
                );
            }
        }
        if let Some(rules) = *self.rules.borrow()
            && let Ok(text) = GetDlgItem(Some(rules), RULES_TEXT)
        {
            send(
                text,
                WM_SETFONT,
                WPARAM(self.font.get().0 as usize),
                LPARAM(1),
            );
        }
        let _ = DeleteObject(old.into());
        let _ = DeleteObject(old_card.into());
    }

    unsafe fn child(
        &self,
        class: PCWSTR,
        title: &str,
        id: usize,
        style: WINDOW_STYLE,
    ) -> windows::core::Result<HWND> {
        let title = wide(title);
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class,
            PCWSTR(title.as_ptr()),
            WS_CHILD | WS_VISIBLE | style,
            0,
            0,
            0,
            0,
            Some(self.hwnd),
            Some(HMENU(id as *mut _)),
            None,
            None,
        )?;
        send(
            hwnd,
            WM_SETFONT,
            WPARAM(self.font.get().0 as usize),
            LPARAM(0),
        );
        Ok(hwnd)
    }

    fn control(&self, id: usize) -> HWND {
        self.controls
            .iter()
            .find(|(key, _)| *key == id)
            .expect("known control")
            .1
    }

    unsafe fn text(&self, hwnd: HWND, text: &str) {
        let text = wide(&text.replace('\n', "\r\n"));
        let _ = SetWindowTextW(hwnd, PCWSTR(text.as_ptr()));
    }

    unsafe fn place(&self, hwnd: HWND, x: i32, y: i32, width: i32, height: i32) {
        let scale = |v: i32| v * self.dpi.get() as i32 / 96;
        let _ = MoveWindow(hwnd, scale(x), scale(y), scale(width), scale(height), true);
    }

    unsafe fn layout(&self) {
        let mut rect = RECT::default();
        let _ = GetClientRect(self.hwnd, &mut rect);
        let width = rect.right * 96 / self.dpi.get() as i32;
        let height = rect.bottom * 96 / self.dpi.get() as i32;
        let main = width - 300;
        self.place(self.heading, 20, 18, main - 10, 30);
        self.place(self.control(RULES), width - 268, 16, 246, 32);
        self.place(self.contract, 20, 58, main - 10, 70);
        self.place(self.seats, 20, 132, main - 10, 66);
        self.place(self.caption, 20, 204, main - 10, 26);
        for player in 0..4 {
            self.place(
                self.control(TRICK + player),
                20 + player as i32 * (main / 4),
                234,
                main / 4 - 10,
                108,
            );
        }
        self.place(self.prompt, 20, 352, main - 10, 64);
        for (id, x, w) in [
            (BID_CHOICE, 20, 110),
            (BID, 140, 100),
            (PASS, 250, 100),
            (DISCARD, 20, 240),
            (TRUMP, 20, 140),
            (PARTNER, 170, 250),
            (DECLARE, 430, 130),
            (NEXT, 20, 200),
        ] {
            self.place(
                self.control(id),
                x,
                420,
                w,
                if [BID_CHOICE, TRUMP, PARTNER].contains(&id) {
                    400
                } else {
                    34
                },
            );
        }
        let columns = (main / 80).clamp(5, 10);
        let card_width = (main - 10) / columns;
        for index in 0..15 {
            self.place(
                self.control(HAND + index),
                20 + index as i32 % columns * card_width,
                478 + index as i32 / columns * 112,
                card_width - 7,
                104,
            );
        }
        self.place(self.scores, width - 268, 70, 246, 230);
        self.place(self.log, width - 268, 308, 246, height - 365);
        self.place(self.status, 20, height - 47, width - 40, 42);
    }

    unsafe fn options(&self, id: usize, titles: &[String], selected: usize) {
        let hwnd = self.control(id);
        send(hwnd, CB_RESETCONTENT, WPARAM(0), LPARAM(0));
        for title in titles {
            let title = wide(title);
            send(
                hwnd,
                CB_ADDSTRING,
                WPARAM(0),
                LPARAM(title.as_ptr() as isize),
            );
        }
        send(hwnd, CB_SETCURSEL, WPARAM(selected), LPARAM(0));
    }

    unsafe fn choice(&self, id: usize) -> Option<usize> {
        let index = send(self.control(id), CB_GETCURSEL, WPARAM(0), LPARAM(0)).0;
        usize::try_from(index).ok()
    }

    unsafe fn render(&self) {
        let state = self.state.borrow();
        let session = &state.session;
        let view = session.view(0);
        let phase = match view.phase {
            Phase::Bidding => "Bidding",
            Phase::Discard => "Choose five discards",
            Phase::Declare => "Choose trump and partner",
            Phase::Playing => "Play a card",
            Phase::Complete => "Round complete",
            Phase::AllPassed => "Redealing",
        };
        self.text(
            self.heading,
            &format!("KOOR 😛     Round {} · {phase}", session.round),
        );
        let contract = if let (Some(leader), Some(trump), Some(partner)) =
            (view.leader, view.trump, view.partner_card)
        {
            let team_scores = view.known_partner.map_or_else(String::new, |partner| {
                let leader_points = view.captured[leader]
                    + if partner == leader {
                        0
                    } else {
                        view.captured[partner]
                    };
                let opposition_points: u16 = view
                    .captured
                    .iter()
                    .enumerate()
                    .filter(|(player, _)| *player != leader && *player != partner)
                    .map(|(_, points)| *points)
                    .sum();
                format!(
                    "\nLeader/Partner: {leader_points} pts · Opposition: {opposition_points} pts"
                )
            });
            format!(
                "{} bid {} · Trump: {trump} · Partner: {partner}\nKitty {} · * = trump · P = called partner card{team_scores}",
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
                "High bid: {} · 180 points per hand · Play proceeds clockwise",
                view.high_bid
            )
        };
        self.render_contract(&contract, view.trump, view.kitty_has_points);
        self.text(
            self.seats,
            &(0..4)
                .map(|p| {
                    format!(
                        "{}{}: {} cards · {} pts{}",
                        if view.turn == Some(p) { "▶ " } else { "" },
                        NAMES[p],
                        view.hand_sizes[p],
                        view.captured[p],
                        if view.leader == Some(p) {
                            " · Leader"
                        } else if view.revealed_partner == Some(p) {
                            " · Partner"
                        } else {
                            ""
                        }
                    )
                })
                .collect::<Vec<_>>()
                .chunks(2)
                .map(|seats| seats.join("     |     "))
                .collect::<Vec<_>>()
                .join("\n"),
        );
        self.text(
            self.scores,
            &format!(
                "CUMULATIVE SCORES\n\n{}",
                (0..4)
                    .map(|p| format!(
                        "{}: {}    ({} this hand)",
                        NAMES[p], session.totals[p], view.captured[p]
                    ))
                    .collect::<Vec<_>>()
                    .join("\n\n")
            ),
        );
        let log = format_log(&view.events);
        let end = log.replace('\n', "\r\n").encode_utf16().count();
        self.text(self.log, &log);
        send(self.log, EM_SETSEL, WPARAM(end), LPARAM(end as isize));
        send(self.log, EM_SCROLLCARET, WPARAM(0), LPARAM(0));
        let lines = send(self.log, EM_GETLINECOUNT, WPARAM(0), LPARAM(0)).0;
        send(self.log, EM_LINESCROLL, WPARAM(0), LPARAM(lines));
        let previous = view.trick.is_empty() && !view.last_trick.is_empty();
        self.text(
            self.caption,
            &if previous {
                format!(
                    "{} took the last trick",
                    NAMES[view.last_winner.unwrap_or(0)]
                )
            } else {
                format!("Trick {} of 10", (view.tricks_finished + 1).min(10))
            },
        );
        let your_turn = view.turn == Some(0);
        for (id, hwnd) in &self.controls {
            let visible = match *id {
                RULES => true,
                BID | PASS | BID_CHOICE => your_turn && view.phase == Phase::Bidding,
                DISCARD => your_turn && view.phase == Phase::Discard,
                DECLARE | TRUMP | PARTNER => your_turn && view.phase == Phase::Declare,
                NEXT => view.phase == Phase::Complete,
                HAND..=114 => *id - HAND < view.hand.len(),
                TRICK..=203 => true,
                _ => false,
            };
            let _ = ShowWindow(*hwnd, if visible { SW_SHOW } else { SW_HIDE });
            let enabled = match *id {
                HAND..=114 => view.hand.get(*id - HAND).is_some_and(|card| {
                    (your_turn && view.phase == Phase::Discard) || view.legal_cards.contains(card)
                }),
                TRICK..=203 => false,
                DISCARD => state.selected.len() == 5,
                _ => true,
            };
            let _ = EnableWindow(*hwnd, enabled);
            if (HAND..HAND + 15).contains(id)
                && let Some(card) = view.hand.get(*id - HAND)
            {
                self.text(
                    *hwnd,
                    &format!(
                        "{card}, {} points {}{}",
                        card.points(),
                        card_marker(*card, view.trump, view.partner_card),
                        if state.selected.contains(card) {
                            ", selected"
                        } else {
                            ""
                        }
                    ),
                );
            }
            let _ = InvalidateRect(Some(*hwnd), None, true);
        }
        let prompt = if let Some(result) = &view.result {
            format!(
                "{} {}: {} points against a bid of {}. {}\nRound scores: {}",
                NAMES[result.leader],
                if result.partner == result.leader {
                    "played alone".into()
                } else {
                    format!("with {}", NAMES[result.partner])
                },
                result.team_points,
                result.bid,
                if result.made_bid {
                    "Bid made!"
                } else {
                    "Bid missed."
                },
                (0..4)
                    .map(|p| format!("{} {:+}", NAMES[p], result.scores[p]))
                    .collect::<Vec<_>>()
                    .join(" · ")
            )
        } else if !your_turn {
            view.turn
                .map(|p| format!("{} is thinking…", NAMES[p]))
                .unwrap_or_else(|| "Everyone passed. Shuffling a new hand…".into())
        } else {
            match view.phase {
                Phase::Bidding => "Raise in multiples of five, or pass. Passing ends your participation in this auction.".into(),
                Phase::Discard => format!("Select five cards to return to the kitty: {} / 5 selected.\nClick a selected card again to keep it.", state.selected.len()),
                Phase::Declare => "Choose trump (left) and a partner card (right), then Announce.\nCalling your own card means playing alone, kept secret until played.".into(),
                Phase::Playing => "YOUR HAND · Click a legal card to play. Follow the lead suit when possible.".into(),
                _ => String::new(),
            }
        };
        self.text(self.prompt, &prompt);
        if your_turn && view.phase == Phase::Bidding {
            let bids: Vec<_> = ((view.high_bid + 5)..=180)
                .step_by(5)
                .map(|bid| bid.to_string())
                .collect();
            let selected = ((80u16.max(view.high_bid + 5) - view.high_bid - 5) / 5) as usize;
            self.options(BID_CHOICE, &bids, selected);
        }
        if your_turn && view.phase == Phase::Declare {
            let suggestion = crate::ai::choose_action(&view, crate::ai::Style::Balanced);
            let (suit, partner) = if let Action::Declare { trump, partner } = suggestion {
                (trump, partner)
            } else {
                (Suit::Purple, Card::Koor)
            };
            self.options(
                TRUMP,
                &Suit::ALL.map(|s| s.name().to_owned()),
                suit as usize,
            );
            self.options(
                PARTNER,
                &view
                    .partner_choices
                    .iter()
                    .map(|card| {
                        format!(
                            "{card}{}",
                            if view.hand.contains(card) {
                                " (your card)"
                            } else {
                                ""
                            }
                        )
                    })
                    .collect::<Vec<_>>(),
                view.partner_choices
                    .iter()
                    .position(|c| *c == partner)
                    .unwrap_or(0),
            );
        }
        self.text(self.status, state.error.as_deref().or(session.warning.as_deref()).unwrap_or(
            "Koor ranks between 10 and 11 of trump · Tab to move between controls · Alt+H for rules"));
    }

    unsafe fn render_contract(&self, text: &str, trump: Option<Suit>, kitty: Option<bool>) {
        self.text(self.contract, text);
        // Reset formatting inherited from the previous contract before adding spans.
        let normal = CHARFORMAT2W {
            Base: CHARFORMATW {
                cbSize: size_of::<CHARFORMAT2W>() as u32,
                dwMask: CFM_COLOR | CFM_BACKCOLOR | CFM_BOLD,
                dwEffects: CFE_AUTOCOLOR | CFE_AUTOBACKCOLOR,
                ..Default::default()
            },
            ..Default::default()
        };
        send(
            self.contract,
            EM_SETCHARFORMAT,
            WPARAM(SCF_ALL as usize),
            LPARAM((&normal as *const CHARFORMAT2W) as isize),
        );
        if let Some(trump) = trump {
            let color = match trump {
                Suit::Purple => 0x00a23671,
                Suit::Pink => 0x00692fae,
                Suit::Orange => 0x000948a5,
                Suit::Brown => 0x00224070,
            };
            self.contract_span(
                text,
                &format!("Trump: {trump}"),
                COLORREF(color),
                COLORREF(0x00eaf5fa),
            );
            self.contract_span(
                text,
                if kitty == Some(true) {
                    "Kitty has points"
                } else {
                    "Kitty has no points"
                },
                COLORREF(0x00ffffff),
                COLORREF(if kitty == Some(true) {
                    0x003c6624
                } else {
                    0x003b3b92
                }),
            );
        }
        send(self.contract, EM_SETSEL, WPARAM(0), LPARAM(0));
    }

    unsafe fn contract_span(
        &self,
        text: &str,
        span: &str,
        foreground: COLORREF,
        background: COLORREF,
    ) {
        let Some(start) = text.find(span) else {
            return;
        };
        // Rich Edit normalizes CRLF to a single CR; selections count UTF-16 units.
        let start = text[..start].replace('\n', "\r").encode_utf16().count();
        let range = CHARRANGE {
            cpMin: start as i32,
            cpMax: (start + span.encode_utf16().count()) as i32,
        };
        send(
            self.contract,
            EM_EXSETSEL,
            WPARAM(0),
            LPARAM((&range as *const CHARRANGE) as isize),
        );
        let format = CHARFORMAT2W {
            Base: CHARFORMATW {
                cbSize: size_of::<CHARFORMAT2W>() as u32,
                dwMask: CFM_COLOR | CFM_BACKCOLOR | CFM_BOLD,
                dwEffects: CFE_BOLD,
                crTextColor: foreground,
                ..Default::default()
            },
            crBackColor: background,
            ..Default::default()
        };
        send(
            self.contract,
            EM_SETCHARFORMAT,
            WPARAM(SCF_SELECTION as usize),
            LPARAM((&format as *const CHARFORMAT2W) as isize),
        );
    }

    unsafe fn command(&self, id: usize) {
        if id == RULES {
            self.show_rules();
            return;
        }
        let view = self.state.borrow().session.view(0);
        let action = match id {
            BID if view.phase == Phase::Bidding && view.turn == Some(0) => self
                .choice(BID_CHOICE)
                .map(|i| Action::Bid(view.high_bid + 5 + i as u16 * 5)),
            PASS => Some(Action::Pass),
            DISCARD => Some(Action::Discard(
                self.state.borrow().selected.iter().copied().collect(),
            )),
            DECLARE => self
                .choice(TRUMP)
                .and_then(|i| Suit::ALL.get(i).copied())
                .zip(
                    self.choice(PARTNER)
                        .and_then(|i| view.partner_choices.get(i).copied()),
                )
                .map(|(trump, partner)| Action::Declare { trump, partner }),
            HAND..=114 => view.hand.get(id - HAND).copied().and_then(|card| {
                if view.phase == Phase::Discard && view.turn == Some(0) {
                    let mut state = self.state.borrow_mut();
                    if !state.selected.remove(&card) && state.selected.len() < 5 {
                        state.selected.insert(card);
                    }
                    None
                } else {
                    Some(Action::Play(card))
                }
            }),
            NEXT if view.phase == Phase::Complete => {
                self.state.borrow_mut().next_round();
                None
            }
            _ => return,
        };
        if let Some(action) = action {
            self.state.borrow_mut().act(action);
        }
        self.render();
        if !IsWindowVisible(GetFocus()).as_bool() {
            let _ = SetFocus(Some(self.control(RULES)));
        }
    }

    unsafe fn draw_card(&self, item: &DRAWITEMSTRUCT) {
        let state = self.state.borrow();
        let view = state.session.view(0);
        let id = item.CtlID as usize;
        let (card, title) = if (HAND..HAND + 15).contains(&id) {
            (view.hand.get(id - HAND).copied(), String::new())
        } else if (TRICK..TRICK + 4).contains(&id) {
            let trick = if view.trick.is_empty() {
                &view.last_trick
            } else {
                &view.trick
            };
            (
                trick
                    .iter()
                    .find(|(p, _)| *p == id - TRICK)
                    .map(|(_, card)| *card),
                NAMES[id - TRICK].into(),
            )
        } else {
            return;
        };
        let selected = id < TRICK && card.is_some_and(|c| state.selected.contains(&c));
        let saved = SaveDC(item.hDC);
        let background = CreateSolidBrush(if selected {
            COLORREF(0x00ffe8d5)
        } else if id < TRICK && item.itemState.0 & ODS_DISABLED.0 != 0 {
            COLORREF(0x00e9e9e9)
        } else {
            COLORREF(0x00f5f7fa)
        });
        FillRect(item.hDC, &item.rcItem, background);
        let _ = DeleteObject(background.into());
        let border = CreateSolidBrush(if selected {
            COLORREF(0x00d07020)
        } else {
            COLORREF(0x00c5c5c5)
        });
        FrameRect(item.hDC, &item.rcItem, border);
        let _ = DeleteObject(border.into());
        SetBkMode(item.hDC, TRANSPARENT);
        SelectObject(item.hDC, self.card_font.get().into());
        let color = match card.map(|c| c.suit(view.trump.unwrap_or(Suit::Purple))) {
            Some(Suit::Purple) => 0x00982f68,
            Some(Suit::Pink) => 0x00602aaa,
            Some(Suit::Orange) => 0x000d489c,
            Some(Suit::Brown) => 0x002b4770,
            None => 0x00606060,
        };
        SetTextColor(item.hDC, COLORREF(color));
        let text = card
            .map(|card| {
                format!(
                    "{}{}{}\n{}\n{} pts {}{}",
                    title,
                    if title.is_empty() { "" } else { "\n" },
                    card.face(),
                    match card {
                        Card::Number(suit, _) => suit.name(),
                        Card::Koor => "KOOR",
                    },
                    card.points(),
                    card_marker(card, view.trump, view.partner_card),
                    if selected { " ✓" } else { "" }
                )
            })
            .unwrap_or_else(|| format!("{title}\n—"));
        let mut text = wide(&text);
        let length = text.len() - 1;
        let mut rect = item.rcItem;
        rect.top += 5;
        DrawTextW(
            item.hDC,
            &mut text[..length],
            &mut rect,
            DT_CENTER | DT_WORDBREAK | DT_NOPREFIX,
        );
        if item.itemState.0 & ODS_FOCUS.0 != 0 {
            let mut focus = item.rcItem;
            let _ = InflateRect(&mut focus, -3, -3);
            let _ = DrawFocusRect(item.hDC, &focus);
        }
        let _ = RestoreDC(item.hDC, saved);
    }

    unsafe fn show_rules(&self) {
        if let Some(hwnd) = *self.rules.borrow()
            && IsWindow(Some(hwnd)).as_bool()
        {
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = SetForegroundWindow(hwnd);
            return;
        }
        let result = (|| -> windows::core::Result<HWND> {
            let hwnd = CreateWindowExW(
                WS_EX_CONTROLPARENT,
                w!("Koor.Table"),
                w!("How to play Koor"),
                WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                700,
                680,
                Some(self.hwnd),
                None,
                None,
                None,
            )?;
            let text = CreateWindowExW(
                WS_EX_CLIENTEDGE,
                w!("EDIT"),
                w!(""),
                WS_CHILD
                    | WS_VISIBLE
                    | WS_VSCROLL
                    | WS_TABSTOP
                    | WINDOW_STYLE((ES_MULTILINE | ES_READONLY | ES_AUTOVSCROLL) as u32),
                0,
                0,
                0,
                0,
                Some(hwnd),
                Some(HMENU(RULES_TEXT as usize as *mut _)),
                None,
                None,
            );
            let text = match text {
                Ok(text) => text,
                Err(error) => {
                    let _ = DestroyWindow(hwnd);
                    return Err(error);
                }
            };
            send(
                text,
                WM_SETFONT,
                WPARAM(self.font.get().0 as usize),
                LPARAM(0),
            );
            send(text, EM_SETLIMITTEXT, WPARAM(100_000), LPARAM(0));
            self.text(text, concat!(include_str!("../../RULES.md"),
                "\n\nImplementation conventions: Passing removes you from the auction; all-pass hands are redealt. A missed bid scores minus the bid for the Leader and partner. The opposition scores captured points. Discarded cards cannot be called as partner."));
            let mut rect = RECT::default();
            let _ = GetClientRect(hwnd, &mut rect);
            let _ = MoveWindow(text, 12, 12, rect.right - 24, rect.bottom - 24, true);
            let _ = ShowWindow(hwnd, SW_SHOW);
            Ok(hwnd)
        })();
        match result {
            Ok(hwnd) => *self.rules.borrow_mut() = Some(hwnd),
            Err(error) => {
                self.state.borrow_mut().error = Some(format!("Could not open rules: {error}"));
                self.render();
            }
        }
    }
}

impl Drop for Table {
    fn drop(&mut self) {
        unsafe {
            // Also cleans up controls when construction fails partway through.
            SetWindowLongPtrW(self.hwnd, GWLP_USERDATA, 0);
            if IsWindow(Some(self.hwnd)).as_bool() {
                let _ = DestroyWindow(self.hwnd);
            }
            let _ = DeleteObject(self.font.get().into());
            let _ = DeleteObject(self.card_font.get().into());
        }
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let pointer = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const Table;
    if let Some(table) = pointer.as_ref() {
        match message {
            WM_COMMAND if (wparam.0 >> 16) == BN_CLICKED as usize => {
                table.command(wparam.0 & 0xffff);
                return LRESULT(0);
            }
            WM_TIMER if wparam.0 == TIMER => {
                let changed = table.state.borrow_mut().tick();
                if changed {
                    table.render();
                }
                return LRESULT(0);
            }
            WM_DRAWITEM => {
                table.draw_card(&*(lparam.0 as *const DRAWITEMSTRUCT));
                return LRESULT(1);
            }
            WM_CTLCOLORSTATIC => {
                let dc = HDC(wparam.0 as *mut _);
                SetBkColor(dc, COLORREF(GetSysColor(COLOR_WINDOW)));
                SetTextColor(dc, COLORREF(GetSysColor(COLOR_WINDOWTEXT)));
                return LRESULT(GetSysColorBrush(COLOR_WINDOW).0 as isize);
            }
            WM_SIZE => {
                table.layout();
                return LRESULT(0);
            }
            WM_GETMINMAXINFO => {
                let info = &mut *(lparam.0 as *mut MINMAXINFO);
                info.ptMinTrackSize = POINT {
                    x: 1000 * table.dpi.get() as i32 / 96,
                    y: 800 * table.dpi.get() as i32 / 96,
                };
                return LRESULT(0);
            }
            WM_DPICHANGED => {
                table.dpi.set((wparam.0 & 0xffff) as u32);
                table.update_fonts();
                let rect = &*(lparam.0 as *const RECT);
                let _ = SetWindowPos(
                    hwnd,
                    None,
                    rect.left,
                    rect.top,
                    rect.right - rect.left,
                    rect.bottom - rect.top,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
                table.layout();
                return LRESULT(0);
            }
            WM_DESTROY => {
                let _ = KillTimer(Some(hwnd), TIMER);
                PostQuitMessage(0);
                return LRESULT(0);
            }
            WM_NCDESTROY => {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            }
            _ => {}
        }
    } else if message == WM_SIZE {
        // Rules windows have no Table pointer and one resizable text control.
        if let Ok(text) = GetDlgItem(Some(hwnd), RULES_TEXT) {
            let _ = MoveWindow(
                text,
                12,
                12,
                (lparam.0 & 0xffff) as i32 - 24,
                ((lparam.0 >> 16) & 0xffff) as i32 - 24,
                true,
            );
        }
    }
    DefWindowProcW(hwnd, message, wparam, lparam)
}

#[cfg(test)]
mod tests {
    use super::*;

    unsafe fn preview(table: &Table, name: &str) {
        let Some(directory) = std::env::var_os("KOOR_SCREENSHOT_DIR") else {
            return;
        };
        let _ = ShowWindow(table.hwnd, SW_SHOWNOACTIVATE);
        let _ = UpdateWindow(table.hwnd);
        let mut rect = RECT::default();
        GetWindowRect(table.hwnd, &mut rect).unwrap();
        rect.right -= rect.left;
        rect.bottom -= rect.top;
        let screen = GetDC(None);
        let dc = CreateCompatibleDC(Some(screen));
        let bitmap = CreateCompatibleBitmap(screen, rect.right, rect.bottom);
        let old = SelectObject(dc, bitmap.into());
        send(
            table.hwnd,
            WM_PRINT,
            WPARAM(dc.0 as usize),
            LPARAM((PRF_CLIENT | PRF_NONCLIENT | PRF_CHILDREN | PRF_ERASEBKGND) as isize),
        );
        SelectObject(dc, old);
        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: rect.right,
                biHeight: rect.bottom,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut pixels = vec![0u8; (rect.right * rect.bottom * 4) as usize];
        assert_ne!(
            GetDIBits(
                dc,
                bitmap,
                0,
                rect.bottom as u32,
                Some(pixels.as_mut_ptr().cast()),
                &mut info,
                DIB_RGB_COLORS
            ),
            0
        );
        let mut bytes = Vec::new();
        bytes.extend(b"BM");
        bytes.extend((54 + pixels.len() as u32).to_le_bytes());
        bytes.extend(0u32.to_le_bytes());
        bytes.extend(54u32.to_le_bytes());
        bytes.extend(40u32.to_le_bytes());
        bytes.extend(rect.right.to_le_bytes());
        bytes.extend(rect.bottom.to_le_bytes());
        bytes.extend(1u16.to_le_bytes());
        bytes.extend(32u16.to_le_bytes());
        bytes.extend([0u8; 24]);
        bytes.extend(pixels);
        std::fs::write(
            std::path::PathBuf::from(directory).join(format!("{name}.bmp")),
            bytes,
        )
        .unwrap();
        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(dc);
        ReleaseDC(None, screen);
        let _ = ShowWindow(table.hwnd, SW_HIDE);
    }

    // Exercise real HWND notifications, dropdown values, button sensitivity,
    // owner drawing, and timer messages. No automated human decisions in production.
    #[test]
    #[ignore = "requires a Windows desktop"]
    fn windows_controls_play_complete_rounds() {
        unsafe {
            let instance = GetModuleHandleW(None).unwrap();
            let class = WNDCLASSW {
                lpfnWndProc: Some(window_proc),
                hInstance: instance.into(),
                lpszClassName: w!("Koor.Table"),
                hbrBackground: GetSysColorBrush(COLOR_WINDOW),
                ..Default::default()
            };
            assert_ne!(RegisterClassW(&class), 0);
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                class.lpszClassName,
                w!("Koor test"),
                WS_OVERLAPPEDWINDOW,
                0,
                0,
                1180,
                880,
                None,
                None,
                Some(instance.into()),
                None,
            )
            .unwrap();
            let table =
                Box::new(Table::new(hwnd, Session::new(42, Some(0), None).unwrap()).unwrap());
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, (&*table as *const Table) as isize);
            let dpi = table.dpi.get() as i32;
            SetWindowPos(
                hwnd,
                None,
                0,
                0,
                1180 * dpi / 96,
                880 * dpi / 96,
                SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
            )
            .unwrap();
            table.layout();
            table.render();
            let click = |id| {
                send(
                    hwnd,
                    WM_COMMAND,
                    WPARAM(id),
                    LPARAM(table.control(id).0 as isize),
                );
            };
            // Make the human win a contract so all setup controls are exercised.
            while table.state.borrow().session.view(0).turn != Some(0) {
                let mut state = table.state.borrow_mut();
                let turn = state.session.view(0).turn.unwrap();
                state.session.act(turn, Action::Pass).unwrap();
            }
            table.render();
            let count = send(table.control(BID_CHOICE), CB_GETCOUNT, WPARAM(0), LPARAM(0)).0;
            preview(&table, "bidding");
            send(
                table.control(BID_CHOICE),
                CB_SETCURSEL,
                WPARAM(count as usize - 1),
                LPARAM(0),
            );
            click(BID);
            assert_eq!(table.state.borrow().session.view(0).phase, Phase::Discard);
            preview(&table, "discard");
            assert!(
                !windows::Win32::UI::Input::KeyboardAndMouse::IsWindowEnabled(
                    table.control(DISCARD)
                )
                .as_bool()
            );
            for index in 0..5 {
                click(HAND + index);
            }
            assert!(
                windows::Win32::UI::Input::KeyboardAndMouse::IsWindowEnabled(
                    table.control(DISCARD)
                )
                .as_bool()
            );
            click(HAND + 5); // selection stays capped at five
            assert_eq!(table.state.borrow().selected.len(), 5);
            click(HAND); // deselect/reselect
            assert_eq!(table.state.borrow().selected.len(), 4);
            click(HAND);
            click(DISCARD);
            assert_eq!(table.state.borrow().session.view(0).phase, Phase::Declare);
            preview(&table, "declare");
            send(table.control(TRUMP), CB_SETCURSEL, WPARAM(2), LPARAM(0));
            let called = table.state.borrow().session.view(0).partner_choices
                [table.choice(PARTNER).unwrap()];
            click(DECLARE);
            let view = table.state.borrow().session.view(0);
            assert_eq!(view.trump, Some(Suit::Orange));
            assert_eq!(view.partner_card, Some(called));
            preview(&table, "playing");

            for _ in 0..200 {
                let view = table.state.borrow().session.view(0);
                if view.phase == Phase::Complete {
                    break;
                }
                if view.turn == Some(0) {
                    let card = view.legal_cards[0];
                    let index = view.hand.iter().position(|c| *c == card).unwrap();
                    click(HAND + index);
                } else {
                    table.state.borrow_mut().next_bot = Instant::now();
                    send(hwnd, WM_TIMER, WPARAM(TIMER), LPARAM(0));
                }
            }
            let view = table.state.borrow().session.view(0);
            assert_eq!(view.phase, Phase::Complete);
            let mut contract_text = [0u16; 2048];
            let length = GetWindowTextW(table.contract, &mut contract_text);
            let contract_text = String::from_utf16_lossy(&contract_text[..length as usize]);
            let team_points = view.result.as_ref().unwrap().team_points;
            assert!(contract_text.contains(&format!("Leader/Partner: {team_points} pts")));
            assert!(contract_text.contains(&format!("Opposition: {} pts", 180 - team_points)));
            preview(&table, "complete");
            assert_eq!(view.tricks_finished, 10);
            assert_eq!(view.captured.iter().sum::<u16>(), 180);
            assert_eq!(
                table.state.borrow().session.totals,
                view.result.unwrap().scores.map(i64::from)
            );
            click(NEXT);
            assert_eq!(table.state.borrow().session.round, 2);
            assert_eq!(table.state.borrow().session.view(0).phase, Phase::Bidding);
            // All-pass auctions redeal without counting a completed round.
            loop {
                let turn = table.state.borrow().session.view(0).turn;
                let Some(turn) = turn else {
                    break;
                };
                table
                    .state
                    .borrow_mut()
                    .session
                    .act(turn, Action::Pass)
                    .unwrap();
            }
            table.state.borrow_mut().next_bot = Instant::now();
            send(hwnd, WM_TIMER, WPARAM(TIMER), LPARAM(0));
            assert_eq!(table.state.borrow().session.view(0).phase, Phase::Bidding);
            assert_eq!(table.state.borrow().session.round, 2);
            // Rules reuse a window, resize, and close without quitting the game.
            click(RULES);
            let rules = table.rules.borrow().unwrap();
            click(RULES);
            assert_eq!(*table.rules.borrow(), Some(rules));
            let _ = SetWindowPos(rules, None, 0, 0, 760, 720, SWP_NOMOVE | SWP_NOZORDER);
            let _ = DestroyWindow(rules);
            assert!(IsWindow(Some(hwnd)).as_bool());
            let mut message = MSG::default();
            assert!(!PeekMessageW(&mut message, None, WM_QUIT, WM_QUIT, PM_REMOVE).as_bool());
            // Paint hand and trick cards into an offscreen DC, including focus.
            let dc = CreateCompatibleDC(None);
            let bitmap = CreateCompatibleBitmap(dc, 200, 150);
            let old = SelectObject(dc, bitmap.into());
            for id in (HAND..HAND + 10).chain(TRICK..TRICK + 4) {
                let item = DRAWITEMSTRUCT {
                    CtlID: id as u32,
                    hDC: dc,
                    rcItem: RECT {
                        left: 0,
                        top: 0,
                        right: 180,
                        bottom: 140,
                    },
                    itemState: ODS_FOCUS,
                    ..Default::default()
                };
                send(
                    hwnd,
                    WM_DRAWITEM,
                    WPARAM(id),
                    LPARAM((&item as *const DRAWITEMSTRUCT) as isize),
                );
            }
            SelectObject(dc, old);
            let _ = DeleteObject(bitmap.into());
            let _ = DeleteDC(dc);
            drop(table);
            assert!(!IsWindow(Some(hwnd)).as_bool());
            UnregisterClassW(class.lpszClassName, Some(instance.into())).unwrap();
        }
    }
}
