# Koor 😛

A Rust implementation of the game in [RULES.md](RULES.md): one human plays against
Ada, Linus, and Grace, with hidden partnerships and 180 points per hand.

The Linux frontend uses native GTK 4 widgets, and the macOS frontend uses native
AppKit widgets through `objc2-app-kit`. Windows uses native Win32 controls through
the `windows` crate. The engine and four-computer CLI work without GUI
dependencies.

## Run on Windows

Install Rust with the MSVC toolchain and Visual Studio Build Tools with the
**Desktop development with C++** workload, then run:

```powershell
cargo run
```

The native window includes bidding, five-card discard selection, trump and partner
dropdowns, colored cards with suit names and trump/partner markers, legal-card
controls, cumulative scores, a scrolling trick log, and a separate rules window.
The info bar highlights trump in its suit color and kitty status in green or red.
Computer turns are paced, with a longer pause after each trick. **Deal next round**
continues after scoring; all-pass auctions redeal automatically. Use Tab and
Shift+Tab to navigate, Space to activate a button, or Alt+H to open the rules.
The window supports resizing and monitor DPI changes.

`cargo build --release` creates `target\release\koor.exe`. Learning state is saved
in `%LOCALAPPDATA%\Koor\learning.json` (or `$XDG_DATA_HOME/koor/learning.json` if
set). `--learning-file` and `--no-learning` also work on Windows.

## Run on macOS

Install Rust and the Xcode command-line tools (`xcode-select --install`), then run:

```sh
cargo run
```

The AppKit window includes colored cards, bidding, five-card discard selection,
trump and partner selectors, legal-card controls, cumulative scores, a scrolling
action log, and the rules. Computer turns are paced, with a longer pause after
each trick. The log groups plays by trick, and hand cards retain suit colors when
disabled. Blue `*` and `P` markers identify trump and the called partner card;
the contract shows colored trump and green/red kitty status. **Deal next round**
continues after scoring. The native menu supports
**⌘Q** to quit, **⌘W** to close, and **⌘M** to minimize. The window follows the
system's light or dark appearance.

To build a Finder-launchable application bundle on macOS 11 or newer:

```sh
./scripts/build-macos-app.sh
open target/release/Koor.app
```

The bundle is built for the Mac's current architecture. It can be copied to
Applications. Learning state is saved in
`~/Library/Application Support/koor/learning.json`; `--learning-file` and
`--no-learning` also work when launching the executable with arguments.

## Run on Linux

Install Rust and GTK 4 development packages (GTK 4.8 or newer), plus `pkg-config`.
For example, Debian/Ubuntu use `libgtk-4-dev` and `pkg-config`; Fedora uses
`gtk4-devel` and `pkgconf-pkg-config`.

```sh
cargo run
```

Bid in increments of five or pass. If you win, select five cards to discard, then
choose trump and a partner card. Click a legal card when it is your turn. The
scoreboard keeps cumulative totals across rounds; **Deal next round** continues
the game. **How to play** opens the rules. The Koor card is a tongue-out face.

## Four-computer CLI

```sh
cargo run --no-default-features -- --cli --rounds 100 --seed 42 --no-learning
cargo run --no-default-features -- --cli --rounds 1 --seed 42 --no-learning --trace
cargo run --no-default-features -- --cli --rounds 1000 --learning-file ./learning.json --quiet
```

`--no-default-features` removes all GUI dependencies. Seats are numbered
0 through 3 in clockwise order. A seed plus `--no-learning` reproduces a game
with the locked dependency versions. `--help` lists the options.

## Rules conventions

The provided rules leave a few cases unspecified. This implementation uses:

- Passing removes a player from that auction. If everyone passes without a bid,
  redeal. A bid of 180 ends the auction immediately.
- Missing the bid scores **minus the bid** for each member of the Leader's team.
  The opposition still scores its combined captured points. Making the bid scores
  the team's combined captured points for each member, as specified in RULES.md.
- A partner card must be in a player's hand after discarding. A discarded card
  cannot be called. Calling your own card is legal and stays secret until played.
- The winner of the last trick takes the kitty points, including when the winner
  belongs to the opposition.

## Computer players and learning

Computers use the strategy notes in [STRATEGY.md](STRATEGY.md): estimate bidding
strength from suit length and controls, evaluate all 3,003 possible kitty
discards, favor void side suits, call strong partner cards, track played cards,
feed known teammates points, and avoid throwing points into losing tricks.
They receive only their own cards and public information.

Bidding gives extra weight to high trumps and side-suit ace/14 pairs, with an
allowance for the kitty and called partner. Each computer's bidding ceiling also
varies by up to ten points per hand, favoring smaller adjustments and staying
fixed throughout the auction. This variation uses the session seed, so seeded
games remain reproducible with learning disabled.

Learning is enabled by default. A contextual epsilon-greedy bandit chooses
between careful, balanced, and bold strategies for weaker and stronger hands.
After each completed round it updates mean rewards from the computer seats'
scores and saves a versioned JSON state using a temporary file and rename. Human
actions are excluded. This is a small reinforcement-learning baseline; improved
playing strength is not yet established, and individual card decisions remain
heuristic.

On Linux the state lives at `$XDG_DATA_HOME/koor/learning.json`, or
`~/.local/share/koor/learning.json` when XDG_DATA_HOME is unset. Override it with
`--learning-file PATH`, or use `--no-learning` for fixed balanced bots and no
persistence. Invalid state files are reported and preserved. Concurrent learning
sessions sharing one file are unsupported. Only learning is persisted; game and
cumulative scores last for the current session.

## Structure and verification

- `cards`: deck, ranking, scoring, suits.
- `game`: validated actions, phases, private player views, tricks, partnerships.
- `ai`: heuristics working exclusively from player views.
- `learning`: persisted strategy selection and rewards.
- `session`: automation, random seed, totals, learning lifecycle.
- `ui`: common `Frontend` interface, CLI, GTK, AppKit, and Win32.

```sh
cargo test
cargo test --no-default-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
```

Engine tests cover special-card ranking and following suit, auctions, validation,
hidden partner reveal, kitty scoring, successful and failed bids, solo play,
seeded complete games, deterministic simulations, human turn pauses, and learning
persistence.

On a Windows desktop, a native interaction test drives the real bid, discard,
declaration, card, and next-round controls through a complete round. It also
checks all-pass redealing, rules window reuse/close, resizing, card drawing,
and native resource cleanup:

```sh
cargo test --lib windows_controls_play_complete_rounds -- --ignored --test-threads=1
```

It skips opening windows during ordinary `cargo test` runs. Setting
`KOOR_SCREENSHOT_DIR` to an existing directory saves BMP phase previews.

An additional GTK interaction test drives the real bid, discard, declaration,
card, and next-round buttons through a complete game. It is ignored by default
because it needs a display. On a desktop, run:

```sh
cargo test --lib gtk_controls_play_a_complete_round -- --ignored --test-threads=1
```

It can also run on an isolated `gtk4-broadwayd` display with
`GDK_BACKEND=broadway BROADWAY_DISPLAY=:5`. Setting `KOOR_SCREENSHOT_DIR` to an
existing directory saves screenshots of the main phases during this test.
