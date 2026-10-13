use koor::{
    ai::{self, Style},
    game::Phase,
    learning::Learning,
    session::Session,
};
use std::collections::HashSet;

#[test]
fn hundreds_of_seeded_games_finish_legally_and_conserve_points() {
    let mut made = false;
    let mut missed = false;
    for seed in 0..256 {
        let mut session = Session::new(seed, None, None).unwrap();
        for _ in 0..300 {
            if matches!(session.game.phase(), Phase::Complete | Phase::AllPassed) {
                break;
            }
            let player = session.game.turn().unwrap();
            let view = session.view(player);
            let action = ai::choose_action(&view, Style::from_index((seed as usize + player) % 3));
            session.act(player, action).unwrap();
        }
        let view = session.view(0);
        assert_eq!(view.phase, Phase::Complete, "seed {seed}");
        assert_eq!(view.tricks_finished, 10);
        assert_eq!(view.hand_sizes, [0; 4]);
        assert_eq!(view.played.len(), 40);
        assert_eq!(view.played.iter().collect::<HashSet<_>>().len(), 40);
        let result = view.result.unwrap();
        assert_eq!(result.captured.iter().sum::<u16>(), 180);
        assert_eq!(session.totals, result.scores.map(i64::from));
        made |= result.made_bid;
        missed |= !result.made_bid;
        assert!(!session.step_bot().unwrap());
        assert_eq!(session.totals, result.scores.map(i64::from));
        session.next_round().unwrap();
        assert_eq!(session.round, 2);
        assert_eq!(session.totals, result.scores.map(i64::from));
    }
    assert!(made && missed);
}

#[test]
fn identical_seed_and_disabled_learning_reproduce_games() {
    let mut first = Session::new(42, None, None).unwrap();
    let mut second = Session::new(42, None, None).unwrap();
    for _ in 0..3 {
        while first.step_bot().unwrap() {
            assert!(second.step_bot().unwrap());
            assert_eq!(first.view(0).events, second.view(0).events);
        }
        assert_eq!(first.view(0).events, second.view(0).events);
        assert_eq!(first.totals, second.totals);
        first.next_round().unwrap();
        second.next_round().unwrap();
    }
}

#[test]
fn human_turns_pause_automation() {
    let mut session = Session::new(42, Some(0), None).unwrap();
    while session.step_bot().unwrap() {}
    assert_eq!(session.game.turn(), Some(0));
    let view = session.view(0);
    assert!(!session.step_bot().unwrap());
    assert_eq!(session.view(0).events, view.events);
    assert!(session.next_round().is_err());
}

#[test]
fn learning_persists_and_resumes_without_recording_human_seat() {
    let directory = std::env::temp_dir().join(format!("koor-test-{}", std::process::id()));
    let path = directory.join("learning.json");
    let mut session = Session::new(7, Some(0), Some(path.clone())).unwrap();
    for _ in 0..300 {
        let Some(player) = session.game.turn() else {
            break;
        };
        if player == 0 {
            let action = ai::choose_action(&session.view(0), Style::Balanced);
            session.act(0, action).unwrap();
        } else {
            assert!(session.step_bot().unwrap());
        }
    }
    assert_eq!(session.game.phase(), Phase::Complete);
    assert!(session.warning.is_none());
    let saved = Learning::load(&path).unwrap();
    assert_eq!(saved.rounds, 1);
    assert_eq!(
        saved.arms.iter().flatten().map(|a| a.visits).sum::<u64>(),
        3
    );
    assert_eq!(
        Session::new(8, None, Some(path.clone()))
            .unwrap()
            .learning
            .rounds,
        1
    );
    std::fs::write(&path, b"invalid learning data").unwrap();
    assert!(Learning::load(&path).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"invalid learning data");
    std::fs::remove_file(path).unwrap();
    std::fs::remove_dir(directory).unwrap();
}
