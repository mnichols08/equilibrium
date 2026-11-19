//! Command log, tick-by-tick sessions and deterministic replay.
//!
//! A [`World`] changes in only two ways: a tick passes, or a command is
//! applied. A [`Session`] drives a world and writes every command to a
//! [`Log`]. Because nothing else can touch the world, [`replay`]ing that
//! log over the same starting world rebuilds the same state and the same
//! events, every time, on every platform.
//!
//! Commands are the record of truth. Events are what the world says
//! happened; they are derived, so they are returned but never stored.
//!
//! # Order within a tick
//!
//! The world starts at tick zero. To reach tick `t`, [`World::tick`] is
//! called once for every tick from the next one up to `t`, in order.
//! Commands stamped `t` are applied after `tick(t)`, in the order they
//! were submitted.
//!
//! ```
//! use eq_clock::Tick;
//! use eq_events::{replay, Session, World};
//!
//! #[derive(Clone, Debug, Default, PartialEq)]
//! struct Counter(u64);
//!
//! impl World for Counter {
//!     type Command = u64;
//!     type Event = &'static str;
//!
//!     fn tick(&mut self, _now: Tick, _events: &mut Vec<Self::Event>) {
//!         self.0 += 1;
//!     }
//!
//!     fn apply(&mut self, amount: &u64, _now: Tick, events: &mut Vec<Self::Event>) {
//!         self.0 += amount;
//!         events.push("added");
//!     }
//! }
//!
//! let mut live = Session::new(Counter::default());
//! live.submit(Tick::new(3), 10).unwrap();
//! live.advance_to(Tick::new(5)).unwrap();
//!
//! let (rebuilt, _events) = replay(Counter::default(), live.log(), live.now());
//! assert_eq!(&rebuilt, live.world());
//! assert_eq!(rebuilt, Counter(15));
//! ```
#![no_std]

extern crate alloc;

use alloc::vec::Vec;
use core::fmt;

use eq_clock::Tick;

/// A simulation that can be driven tick by tick and replayed.
///
/// Both methods must be deterministic: no wall clock, no I/O, no global
/// state, no randomness that is not held inside `self`.
pub trait World {
    /// Something a player or the server asks the world to do.
    type Command;
    /// Something the world reports as having happened.
    type Event;

    /// Runs one tick of world time with no input.
    fn tick(&mut self, now: Tick, events: &mut Vec<Self::Event>);

    /// Applies one command during tick `now`.
    fn apply(&mut self, command: &Self::Command, now: Tick, events: &mut Vec<Self::Event>);
}

/// A value with the tick it belongs to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stamped<T> {
    /// When it happened.
    pub tick: Tick,
    /// What happened.
    pub value: T,
}

/// Returned when something is stamped earlier than the latest tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OutOfOrder {
    /// The tick that was asked for.
    pub attempted: Tick,
    /// The latest tick already reached.
    pub latest: Tick,
}

impl fmt::Display for OutOfOrder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "tick {} is earlier than the latest tick {}",
            self.attempted.get(),
            self.latest.get()
        )
    }
}

impl core::error::Error for OutOfOrder {}

/// An append-only record of commands, in the order they were applied.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Log<C> {
    entries: Vec<Stamped<C>>,
}

impl<C> Default for Log<C> {
    fn default() -> Self {
        Self::new()
    }
}

impl<C> Log<C> {
    /// An empty log.
    pub const fn new() -> Self {
        Self { entries: Vec::new() }
    }

    /// Appends a command. Ticks may repeat but never go backwards.
    pub fn record(&mut self, tick: Tick, command: C) -> Result<(), OutOfOrder> {
        let latest = self.latest_tick();
        if tick < latest {
            return Err(OutOfOrder { attempted: tick, latest });
        }
        self.entries.push(Stamped { tick, value: command });
        Ok(())
    }

    /// Every recorded command, oldest first.
    pub fn entries(&self) -> &[Stamped<C>] {
        &self.entries
    }

    /// How many commands have been recorded.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether nothing has been recorded.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The tick of the newest command, or tick zero for an empty log.
    pub fn latest_tick(&self) -> Tick {
        self.entries.last().map_or(Tick::ZERO, |entry| entry.tick)
    }
}

/// Events produced by a step, each with the tick it happened in.
pub type Events<W> = Vec<Stamped<<W as World>::Event>>;

/// A running world together with the log of everything done to it.
///
/// The only ways to change the world are [`Session::advance_to`] and
/// [`Session::submit`], so the log always matches what happened.
#[derive(Clone, Debug)]
pub struct Session<W: World> {
    world: W,
    log: Log<W::Command>,
    now: Tick,
}

impl<W: World> Session<W> {
    /// Starts a session at tick zero.
    pub const fn new(world: W) -> Self {
        Self { world, log: Log::new(), now: Tick::ZERO }
    }

    /// Picks a world up again where it was saved: at `now`, with nothing
    /// in the log yet. Everything done to it before is in whatever it was
    /// saved from.
    pub const fn resume(world: W, now: Tick) -> Self {
        Self { world, log: Log::new(), now }
    }

    /// The latest tick the world has reached.
    pub const fn now(&self) -> Tick {
        self.now
    }

    /// The world as it stands.
    pub const fn world(&self) -> &W {
        &self.world
    }

    /// Every command applied so far.
    pub const fn log(&self) -> &Log<W::Command> {
        &self.log
    }

    /// Ends the session, handing back the world and its log.
    pub fn into_parts(self) -> (W, Log<W::Command>) {
        (self.world, self.log)
    }

    /// Runs ticks until the world reaches `tick`.
    ///
    /// Work is proportional to the number of ticks covered.
    pub fn advance_to(&mut self, tick: Tick) -> Result<Events<W>, OutOfOrder> {
        let mut events = Vec::new();
        run_ticks(&mut self.world, &mut self.now, tick, &mut events)?;
        Ok(events)
    }

    /// Runs ticks up to `tick`, then records and applies `command`.
    pub fn submit(&mut self, tick: Tick, command: W::Command) -> Result<Events<W>, OutOfOrder> {
        let mut events = Vec::new();
        run_ticks(&mut self.world, &mut self.now, tick, &mut events)?;
        apply_command(&mut self.world, &command, tick, &mut events);
        self.log.record(tick, command)?;
        Ok(events)
    }
}

/// Rebuilds a world by running `log` over `world`, then ticking on to
/// `until`.
///
/// Given the same starting world and log, the result is always the same.
///
/// # Panics
/// Panics if `until` is earlier than the last command in the log.
pub fn replay<W: World>(mut world: W, log: &Log<W::Command>, until: Tick) -> (W, Events<W>) {
    let mut now = Tick::ZERO;
    let mut events = Vec::new();
    for entry in log.entries() {
        // A log only ever holds ticks in order, so this cannot fail.
        run_ticks(&mut world, &mut now, entry.tick, &mut events).expect("log is in tick order");
        apply_command(&mut world, &entry.value, entry.tick, &mut events);
    }
    run_ticks(&mut world, &mut now, until, &mut events).expect("until must not precede the log");
    (world, events)
}

fn run_ticks<W: World>(
    world: &mut W,
    now: &mut Tick,
    target: Tick,
    events: &mut Events<W>,
) -> Result<(), OutOfOrder> {
    if target < *now {
        return Err(OutOfOrder { attempted: target, latest: *now });
    }
    let mut raw = Vec::new();
    while *now < target {
        *now = now.next();
        world.tick(*now, &mut raw);
        stamp(&mut raw, *now, events);
    }
    Ok(())
}

fn apply_command<W: World>(world: &mut W, command: &W::Command, tick: Tick, events: &mut Events<W>) {
    let mut raw = Vec::new();
    world.apply(command, tick, &mut raw);
    stamp(&mut raw, tick, events);
}

fn stamp<E>(raw: &mut Vec<E>, tick: Tick, events: &mut Vec<Stamped<E>>) {
    events.extend(raw.drain(..).map(|value| Stamped { tick, value }));
}

#[cfg(test)]
mod tests {
    use super::*;
    use eq_rng::Rng;

    /// A small world with randomness: a pile that grows by chance each
    /// tick and shrinks when someone takes from it.
    #[derive(Clone, Debug, PartialEq)]
    struct Pile {
        rng: Rng,
        size: u64,
    }

    #[derive(Clone, Debug, PartialEq)]
    enum Happened {
        Grew,
        Took(u64),
    }

    impl World for Pile {
        type Command = u64;
        type Event = Happened;

        fn tick(&mut self, _now: Tick, events: &mut Vec<Happened>) {
            if self.rng.chance(1, 3) {
                self.size += 1;
                events.push(Happened::Grew);
            }
        }

        fn apply(&mut self, wanted: &u64, _now: Tick, events: &mut Vec<Happened>) {
            let taken = (*wanted).min(self.size);
            self.size -= taken;
            events.push(Happened::Took(taken));
        }
    }

    fn pile(seed: u64) -> Pile {
        Pile { rng: Rng::from_seed(seed), size: 0 }
    }

    #[test]
    fn log_refuses_to_go_backwards() {
        let mut log = Log::new();
        log.record(Tick::new(5), 'a').unwrap();
        log.record(Tick::new(5), 'b').unwrap();
        let error = log.record(Tick::new(4), 'c').unwrap_err();
        assert_eq!(error, OutOfOrder { attempted: Tick::new(4), latest: Tick::new(5) });
        assert_eq!(log.len(), 2);
    }

    #[test]
    fn session_ticks_once_per_tick_then_applies() {
        #[derive(Default)]
        struct Trace(Vec<(u64, &'static str)>);
        impl World for Trace {
            type Command = ();
            type Event = ();
            fn tick(&mut self, now: Tick, _: &mut Vec<()>) {
                self.0.push((now.get(), "tick"));
            }
            fn apply(&mut self, _: &(), now: Tick, _: &mut Vec<()>) {
                self.0.push((now.get(), "apply"));
            }
        }

        let mut session = Session::new(Trace::default());
        session.submit(Tick::new(2), ()).unwrap();
        session.submit(Tick::new(2), ()).unwrap();
        session.advance_to(Tick::new(3)).unwrap();
        assert_eq!(
            session.world().0,
            [(1, "tick"), (2, "tick"), (2, "apply"), (2, "apply"), (3, "tick")]
        );
    }

    #[test]
    fn rejected_submit_changes_nothing() {
        let mut session = Session::new(pile(1));
        session.advance_to(Tick::new(10)).unwrap();
        let before = session.world().clone();
        assert!(session.submit(Tick::new(9), 1).is_err());
        assert!(session.advance_to(Tick::new(9)).is_err());
        assert_eq!(session.world(), &before);
        assert!(session.log().is_empty());
        assert_eq!(session.now(), Tick::new(10));
    }

    #[test]
    fn events_carry_their_tick() {
        let mut session = Session::new(pile(7));
        let events = session.submit(Tick::new(50), 2).unwrap();
        let (last, earlier) = events.split_last().unwrap();
        assert_eq!(last.tick, Tick::new(50));
        assert!(matches!(last.value, Happened::Took(_)));
        assert!(earlier.iter().all(|event| event.value == Happened::Grew));
        assert!(earlier.windows(2).all(|pair| pair[0].tick <= pair[1].tick));
    }

    #[test]
    fn replay_rebuilds_identical_state_and_events() {
        let mut live = Session::new(pile(42));
        let mut seen = Vec::new();
        let mut script = Rng::from_seed(9);
        let mut tick = 0;
        for _ in 0..200 {
            tick += script.below(20);
            seen.extend(live.submit(Tick::new(tick), script.below(5)).unwrap());
        }
        seen.extend(live.advance_to(Tick::new(tick + 100)).unwrap());

        let (rebuilt, replayed) = replay(pile(42), live.log(), live.now());
        assert_eq!(&rebuilt, live.world());
        assert_eq!(replayed, seen);
    }

    #[test]
    fn how_ticks_are_batched_does_not_matter() {
        let mut jump = Session::new(pile(3));
        jump.advance_to(Tick::new(1_000)).unwrap();

        let mut crawl = Session::new(pile(3));
        for tick in 1..=1_000 {
            crawl.advance_to(Tick::new(tick)).unwrap();
        }
        assert_eq!(jump.world(), crawl.world());
    }

    #[test]
    fn different_seeds_diverge() {
        let (a, _) = replay(pile(1), &Log::new(), Tick::new(500));
        let (b, _) = replay(pile(2), &Log::new(), Tick::new(500));
        assert_ne!(a, b);
    }
}
