# How learning works in Koor

Koor uses a small reinforcement-learning system called a **contextual
epsilon-greedy bandit**. At the start of a hand, it picks a preset strategy style
for each computer player. After the hand finishes, it uses that player's score
to update how rewarding that choice has been.

The learned policy answers: **“For this kind of starting hand, which style has
earned the best average score?”** Individual bids, discards, declarations, and
card plays still come from handwritten rules in [src/ai.rs](src/ai.rs). There is
no neural network, card-level value table, or separate offline training process.
Ordinary play and four-computer CLI simulations both provide training data.

## Where the code lives

| File | Responsibility |
| --- | --- |
| [src/learning.rs](src/learning.rs) | Stores statistics, selects styles, updates rewards, and loads/saves JSON. |
| [src/session.rs](src/session.rs) | Connects learning to deals, bot turns, completed rounds, and persistence. |
| [src/ai.rs](src/ai.rs) | Estimates hand strength and implements the three heuristic styles. |
| [src/game.rs](src/game.rs) | Enforces legal actions, limits each player's view, and calculates final scores. |
| [src/main.rs](src/main.rs) | Enables learning by default and handles command-line options. |
| [tests/simulation.rs](tests/simulation.rs) | Checks simulations, reproducibility, and learning persistence. |

The heuristics are based on the advice in [STRATEGY.md](STRATEGY.md). The game
rules are in [RULES.md](RULES.md).

## One round, from selection to feedback

1. `Session::new` loads the learning file, seeds its random-number generator,
   deals a game, and calls `select_styles`. Later deals also call `select_styles`.
2. Each seat's initial ten-card hand is assigned a strength context. The learner
   selects a style for that context, and the session remembers `(context, style)`
   in `choices`. It also samples and stores that seat's bidding ceiling.
3. On a computer turn, `step_bot` passes the seat's `View`, selected style, and
   stored bidding ceiling to `ai::choose_action_with_bid_ceiling`. The style
   stays fixed for the entire hand, including after the Leader picks up the kitty.
4. `Session::act` applies the action and calls `record_result`. Until the game
   has a final result, there is no learning update.
5. When the round completes, `record_result` updates cumulative session scores,
   records one observation per computer seat, increments the persisted completed
   round count, and saves the learning file. The `recorded` flag prevents counting
   the same result twice.

An all-pass auction produces no final result and no reward. Redealing selects
fresh styles and bidding ceilings. Closing the app partway through a hand also
provides no completed-round observation.

Bots act from their own cards and public information, including any partner
identity they can legitimately know. They do not receive opponents' hidden hands
during training. Final scores become feedback only after the round ends.

## The context: two kinds of starting hand

`Session::select_styles` calculates:

```rust
let context = usize::from(ai::estimate_bid(&hand) >= 120);
```

| Context index | Starting-hand estimate |
| --- | --- |
| `0` | Below 120: weaker hand |
| `1` | At least 120: stronger hand |

`estimate_bid` chooses a preferred trump suit using suit length and card strength,
then scores trump length, high trumps, aces, side-suit ace/14 pairs, and Koor. It
includes an allowance for the kitty and called partner, clamps its estimate to
80–170, and rounds down to a multiple of five.

This estimate is a heuristic, not a learned prediction. The context is calculated
before adding the style's bid adjustment or random bidding variation. It is not
recalculated after discarding, declaring trump, or revealing a partner.

## The actions: three strategy styles

In bandit terminology, an **arm** is one available choice. Here the arms are the
three values of `ai::Style`:

| Style index | Style | Bid adjustment | Other style-specific behavior |
| --- | --- | --- | --- |
| `0` | Careful | −15 points | Adds a penalty for leading a trump that is not a control. |
| `1` | Balanced | 0 points | Uses the shared declaration and play rules. |
| `2` | Bold | +15 points | May call its own trump ace to play solo when it holds at least six trumps including that ace and the bid is at most 135. |

A control is a card for which every higher card in its suit is already played or
in the bot's own hand. All styles share discard selection, preferred-trump
selection, and most play rules. For example, they all try to feed points to a
known teammate and avoid giving away points in a losing trick. Learning never
changes the numeric weights in those rules.

The bidding ceiling also gets a random adjustment from −10 to +10 in increments
of five. This is the sum of two independent draws from `−1, 0, 1`, multiplied by
five, so small adjustments are more common. The final ceiling is clamped to
5–180 and stays fixed throughout the auction. This variation exists even with
learning disabled; it is separate from exploration of styles.

## How the learner chooses a style

`Learning::select` looks only at the three arms for the current context:

1. If any arm has zero observations, choose the first such arm in index order:
   Careful, then Balanced, then Bold.
2. Once all three have observations, explore with probability `0.12`: pick one
   of the three styles uniformly at random.
3. Otherwise, choose the style with the highest `mean_reward`. The current
   iterator's tie behavior favors the later index, so a three-way tie selects Bold.

The 12% exploration rate remains constant. Exploration can select the best arm
too; with a unique best arm, its total selection probability is 92%, and each
other arm has probability 4%.

All seats share one table of six arms: two contexts times three styles. There
are no separate models for Ada, Linus, Grace, or different opponents. Selections
are made before any seat receives its round-end reward, so several seats in the
same context can choose the same untried arm in one round. An arm's `visits`
counts completed observations, not selections waiting for a result.

## The reward and update

`Learning::observe(context, style, score)` normalizes a seat's final round score:

```text
reward = score / 180
visits = visits + 1
mean_reward = mean_reward + (reward - mean_reward) / visits
```

This computes the arithmetic mean of all rewards observed for that context and
style, without storing their history. For example, scores of 90 and −180 produce
rewards of `0.5` and `−1.0`; after both observations the mean is `−0.25`.

The score comes from `Game::finish_round`, rather than the individual seat's
captured cards or cumulative session total:

| Seat's role and outcome | Score used for learning |
| --- | --- |
| Leader or called partner, bid made | The Leader team's total captured points |
| Leader or called partner, bid missed | Negative of the bid |
| Opposition | `180 - Leader team's captured points` |

Rewards therefore lie between −1 and +1. Both teammates receive their team's
score, including when one captured fewer points personally. If the Leader calls
their own card, the same scoring rule applies to that solo Leader. Seat scores
are not a zero-sum reward scheme.

In the GUI, seat 0 is human and contributes no learning observation. A completed
GUI round normally adds three visits across the table; a four-computer CLI round
adds four. `Learning::rounds` increases by one in either case. Human decisions can
affect the bots' outcomes, but are never recorded as examples of a chosen style.
Internally, the session still selects a style and ceiling for the human seat;
those do not control human actions.

## What is saved

The JSON state contains:

- `version`: currently `1`.
- `rounds`: completed rounds recorded while learning was enabled, across runs.
- `arms`: a two-row, three-column array in the context/style order above. Each
  entry contains `visits` and `mean_reward`.

A fresh table has zero rounds, zero visits, and zero mean rewards. Missing files
start a fresh table. Loading rejects malformed JSON, unsupported versions, and
nonfinite or out-of-range mean rewards. A load failure stops startup and leaves
the existing file intact; `--no-learning` allows playing without loading it.

After each completed round, saving creates parent directories if needed, writes
pretty-printed JSON to a temporary file whose extension contains the process ID,
syncs the file, and renames it to the requested path. A save failure leaves the
in-memory update intact and sets `Session::warning`; the CLI reports that warning.
The next completed round attempts another save. Multiple sessions sharing the
same file are unsupported: there is no locking or merging of observations.

Default paths are checked in this order:

| Condition | Path |
| --- | --- |
| `XDG_DATA_HOME` is set, on any platform | `$XDG_DATA_HOME/koor/learning.json` |
| Windows with `LOCALAPPDATA` set | `%LOCALAPPDATA%\Koor\learning.json` |
| macOS with `HOME` set | `~/Library/Application Support/koor/learning.json` |
| Other platforms with `HOME` set, including Windows fallback | `~/.local/share/koor/learning.json` |
| None of the applicable environment variables is available | `koor-learning.json` in the working directory |

Only these learning statistics persist. The current game, random-generator
state, and cumulative session scores are not saved.

## Running and inspecting learning

Run 1,000 four-computer rounds with an explicit learning file:

```sh
cargo run --no-default-features -- --cli --rounds 1000 --seed 42 --learning-file ./learning.json --quiet
```

Running that command again loads the accumulated statistics and continues
learning. To start an independent experiment, choose a new, nonexistent path.
You can inspect the resulting JSON in a text editor: `visits` shows which choices
received feedback, and `mean_reward` shows their historical average reward.

For reproducible games with the locked dependency versions, disable learning:

```sh
cargo run --no-default-features -- --cli --rounds 100 --seed 42 --no-learning
```

`--no-learning` uses Balanced for all bots and skips learning updates and file
access. It retains seeded bidding variation. With learning enabled, the seed
alone is insufficient to reproduce a run: selection also depends on the initial
learning state. Style exploration and dealing use the same random generator, so
changes in learning can also change subsequent deals.

## What the current results can tell us

The table measures historical scores for broad style choices. Improved playing
strength has not yet been established. A high mean can reflect favorable deals,
partners, opponents, and bidding roles as well as a useful style. Multiple bots
learn together, so their observations are related and their opponents' behavior
can change over time.

The context omits details such as seat role and precise hand composition, and
the entire round's reward is attributed to one style. There is no feedback for
an individual good or bad card play, no update toward a predicted future value,
and no discount factor. The running mean weights every observation equally;
older games are never deliberately forgotten.

The tests check legal complete games, deterministic play with learning disabled,
fixed bidding ceilings during an auction, persistence and resumption, exclusion
of the human seat, and preservation of invalid files. They do not demonstrate a
win-rate improvement. Run the engine and simulation checks without GUI dependencies:

```sh
cargo test --no-default-features
```
