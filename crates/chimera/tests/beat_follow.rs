//! The sequencer following a host's transport: each step once, at its time, whatever the host's
//! blocks and positions look like. See docs/plugin.md, "Beatbox".
use chimera::control::beat_player::BeatPlayer;
use chimera::phonetics::beatbox::{BeatStep, parse_beat};

const FS: f64 = 44100.0;
const DELAY: f64 = 0.18;

fn player(pattern: &str) -> BeatPlayer {
    chimera::phonetics::beatbox::init();
    let mut p = BeatPlayer::new();
    p.set_steps(&parse_beat(pattern));
    p
}

/// A kick on every step, so a scheduled sound and a marked step are the same thing.
fn all_kicks() -> BeatPlayer {
    let mut p = player("");
    p.set_steps(&[Some(BeatStep::new("B")); 16]);
    p
}

/// What a run of blocks scheduled: step and hit time, and how many sounds went into the score.
#[derive(Default)]
struct Log {
    hits: Vec<(i64, f64)>,
    sounds: usize,
}

impl Log {
    fn block(&mut self, p: &mut BeatPlayer, clock: f64, beats: f64, samples: usize, bpm: f64) {
        let before: Vec<_> = p.recent_hits().collect();
        let end = beats + samples as f64 / FS * bpm / 60.0;
        let mut sounds = 0;
        p.follow(clock, beats, end, bpm, DELAY, &mut |_| sounds += 1);
        self.sounds += sounds;
        let mut new: Vec<_> = p.recent_hits().filter(|h| !before.contains(h)).collect();
        new.sort_by_key(|h| h.0);
        self.hits.extend(new);
    }
}

/// A steady host from `beats`, in blocks from `size`.
fn steady(p: &mut BeatPlayer, log: &mut Log, clock: &mut f64, mut beats: f64, seconds: f64, bpm: f64, size: impl Fn(usize) -> usize) {
    let mut left = (seconds * FS) as usize;
    let mut k = 0;
    while left > 0 {
        let n = size(k).min(left);
        log.block(p, *clock, beats, n, bpm);
        *clock += n as f64 / FS;
        beats += n as f64 / FS * bpm / 60.0;
        left -= n;
        k += 1;
    }
}

#[test]
fn each_step_once_at_its_time_whatever_the_blocks() {
    let run = |size: fn(usize) -> usize| {
        let mut p = all_kicks();
        let mut log = Log::default();
        let mut clock = 10.0;
        steady(&mut p, &mut log, &mut clock, 0.0, 2.0, 120.0, size);
        log
    };
    let reference = run(|_| 512);
    assert_eq!(reference.hits.iter().map(|h| h.0).collect::<Vec<_>>(), (0..16).collect::<Vec<_>>());
    assert_eq!(reference.sounds, 16);
    for (n, t) in &reference.hits {
        let want = 10.0 + *n as f64 * 0.125 + DELAY;
        assert!((t - want).abs() < 1e-9, "step {n} at {t}, want {want}");
    }
    let sizes: [fn(usize) -> usize; 3] = [|_| 64, |_| 4096, |k| [17, 1024, 3, 256, 999][k % 5]];
    for size in sizes {
        let other = run(size);
        assert_eq!(other.sounds, reference.sounds);
        for (a, b) in reference.hits.iter().zip(&other.hits) {
            assert_eq!(a.0, b.0);
            assert!((a.1 - b.1).abs() < 1e-9, "the same step lands in the same place: {a:?} {b:?}");
        }
    }
}

#[test]
fn starting_mid_song_picks_up_at_the_next_step() {
    for (beats, first) in [(1.0, 4), (1.03, 5), (1.2499, 5), (1.2501, 6), (0.0, 0)] {
        let mut p = all_kicks();
        let mut log = Log::default();
        log.block(&mut p, 0.0, beats, 44100, 120.0);
        assert_eq!(log.hits[0].0, first, "from beat {beats}");
    }
}

#[test]
fn a_pre_roll_waits_for_beat_zero() {
    let mut p = all_kicks();
    let mut log = Log::default();
    let mut clock = 0.0;
    steady(&mut p, &mut log, &mut clock, -1.0, 0.45, 120.0, |_| 512);
    assert!(log.hits.is_empty(), "nothing before beat 0: {:?}", log.hits);
    let at = -1.0 + clock * 2.0;
    steady(&mut p, &mut log, &mut clock, at, 0.2, 120.0, |_| 512);
    assert_eq!(log.hits[0].0, 0);
    assert!((log.hits[0].1 - (0.5 + DELAY)).abs() < 1e-9, "beat 0 is half a second in: {:?}", log.hits[0]);
    assert_eq!(p.step_at(log.hits[0].1), 0, "and the pattern's first step, not a negative one");
}

#[test]
fn a_loop_starts_the_steps_again_with_nothing_doubled_or_lost() {
    let mut p = all_kicks();
    let mut log = Log::default();
    let mut clock = 0.0;
    for _ in 0..3 {
        // one bar at 120 bpm
        steady(&mut p, &mut log, &mut clock, 0.0, 2.0, 120.0, |_| 512);
    }
    assert_eq!(log.hits.len(), 48);
    assert_eq!(log.sounds, 48);
    for (i, (n, t)) in log.hits.iter().enumerate() {
        assert_eq!(*n, (i % 16) as i64);
        assert!((t - (i as f64 * 0.125 + DELAY)).abs() < 1e-6, "hit {i} at {t}");
    }
}

#[test]
fn a_seek_looks_the_step_up_again() {
    let mut p = all_kicks();
    let mut log = Log::default();
    let mut clock = 0.0;
    steady(&mut p, &mut log, &mut clock, 0.0, 0.3, 120.0, |_| 512);
    let n = log.hits.len();
    steady(&mut p, &mut log, &mut clock, 8.0, 0.3, 120.0, |_| 512);
    assert_eq!(log.hits[n].0, 32, "beat 8 is step 32");
    assert_eq!(p.steps()[(log.hits[n].0 % 16) as usize], Some(BeatStep::new("B")));
}

/// Found by review: a host that repeats its position for a block looked like a seek to a fixed
/// threshold, and could double a step.
#[test]
fn a_repeated_or_uneven_position_cant_double_or_lose_a_step() {
    let bpm = 120.0;
    let n = 1024;
    let per_block = n as f64 / FS * bpm / 60.0;
    let mut p = all_kicks();
    let mut log = Log::default();
    let (mut clock, mut beats) = (0.0, 0.0);
    for k in 0..200 {
        // every seventh block the host repeats itself and then catches up; others wobble a little
        let reported = if k % 7 == 3 { beats - per_block } else { beats + 2e-4 * ((k * 13 % 5) as f64 - 2.0) };
        log.block(&mut p, clock, reported, n, bpm);
        clock += n as f64 / FS;
        beats += per_block;
    }
    let steps: Vec<_> = log.hits.iter().map(|h| h.0).collect();
    assert_eq!(steps, (0..steps.len() as i64).collect::<Vec<_>>(), "each step once, in order");
    assert!(steps.len() as f64 > beats * 4.0 - 2.0, "and none lost: {} steps in {beats} beats", steps.len());
    // however late it was noticed, a step keeps its time on the timeline, to within the wobble
    for (n, t) in &log.hits {
        let want = *n as f64 * 0.125 + DELAY;
        assert!((t - want).abs() < 0.03, "step {n} at {t}, want {want}");
    }
}

#[test]
fn swing_delays_the_odd_steps_and_tempo_can_change_at_any_block() {
    let mut p = all_kicks();
    p.swing = 0.4;
    let mut log = Log::default();
    let mut clock = 0.0;
    steady(&mut p, &mut log, &mut clock, 0.0, 1.0, 120.0, |_| 256);
    for (n, t) in &log.hits {
        let swing = if n % 2 == 1 { 0.4 * 0.5 * 0.125 } else { 0.0 };
        assert!((t - (*n as f64 * 0.125 + swing + DELAY)).abs() < 1e-9, "step {n} at {t}");
    }
    // a jump to 60 bpm where the host says: the steps carry on, at the new spacing
    let mut p = all_kicks();
    let mut log = Log::default();
    let mut clock = 0.0;
    steady(&mut p, &mut log, &mut clock, 0.0, 1.0, 120.0, |_| 256);
    let split = log.hits.len();
    let at = clock * 2.0;
    steady(&mut p, &mut log, &mut clock, at, 1.0, 60.0, |_| 256);
    assert_eq!(log.hits[split].0, split as i64, "nothing lost at the change");
    let gap = log.hits[split + 2].1 - log.hits[split + 1].1;
    assert!((gap - 0.25).abs() < 1e-9, "a sixteenth at 60 bpm: {gap}");
    assert_eq!(p.bpm, 60.0);
}

#[test]
fn rests_are_steps_too_and_the_playhead_follows() {
    let mut p = player("B . t .");
    let mut log = Log::default();
    let mut clock = 0.0;
    steady(&mut p, &mut log, &mut clock, 0.0, 1.0, 120.0, |_| 256);
    assert_eq!(log.hits.len(), 8);
    assert_eq!(log.sounds, 4, "two of each four steps are rests");
    // a second in, with 180 ms of delay, the last hit heard is step 6: the pattern's step 2
    assert_eq!(p.step_at(clock), 2);
    assert_eq!(p.step_at(0.0), -1, "nothing heard yet at the start");
    assert!(p.following());
    p.stop_following();
    assert_eq!(p.step_at(clock), -1);
    assert!(!p.following());
    // an empty pattern does nothing, and doesn't lose its place
    let mut p = player("");
    p.set_steps(&[]);
    let mut sounds = 0;
    p.follow(0.0, 0.0, 1.0, 120.0, DELAY, &mut |_| sounds += 1);
    assert_eq!(sounds, 0);
}
