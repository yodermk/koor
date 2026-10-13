This will be a Rust application to play a Rook-like game, with one human player playing against 3 computer players.

See RULES.md for the game rules and STRATEGY.md for some ideas on how computer players should think.

Make this a multi-playtform program with native UI on each platform. Use the `gtk4` crate on Linux,
`windows` for Windows, and `objc2-app-kit` for macOS.

Design and implement, for each native UI, a program that allows the human player to play against the computer players.
Make the interface as nice as possible.

Abstract the UI interface so it can be implemented separately for each platform. Also make a CLI-only version
with 4 computer players and no human, just for testing.

Implement computer AI, and if possible implement some kind of reinforcement learning algorithm and save
the learning state, so that it keeps improving over time.

The Koor card should be represented by a face with a tongue sticking out.
