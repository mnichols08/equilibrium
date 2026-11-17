//! Simple characters that play by themselves.
//!
//! A bot follows a [`Plan`]: a route to walk and a [`Policy`] for when to
//! gather. Bots act through ordinary commands, so everything they do is
//! in the log and replays like any player's moves.
//!
//! They exist for two jobs: testing what crowds do to a zone, and giving
//! a lone player some company to share it with.

use alloc::vec::Vec;

use eq_clock::{Duration, Tick};
use eq_events::{Events, OutOfOrder, Session};
use eq_nodes::Quality;

use crate::content::{CIRCUIT, DEPOSIT_ROUTES, FULL_REST, MIN_WORKABLE_STOCK};
use crate::{Character, CharacterId, Command, Death, Event, Game, Outcome, Safety, Standing};

/// A warden with less health than this leaves fighting to others.
const HEALTH_TO_FIGHT: u32 = 40;

/// How long a bot stands idle when there is nothing it wants to do.
const IDLE: Duration = Duration::seconds(5);

/// How long a bot waits before trying a flooded way back again.
const TIDE_WAIT: Duration = Duration::minutes(1);

/// The least time between a bot's turns, so a route of dead ends cannot
/// spin without the clock moving.
const PAUSE: Duration = Duration::seconds(1);

/// When a bot gathers from the deposit in front of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Policy {
    /// Takes whatever can be taken.
    Greedy,
    /// Takes only from deposits that are pristine and at least half full.
    Careful,
}

impl Policy {
    /// Whether a bot with this policy would gather where `who` stands.
    pub fn wants_to_gather(self, game: &Game, who: CharacterId) -> bool {
        let Some(node) = game.place_of(who).and_then(|place| place.deposit).map(|deposit| deposit.node) else {
            return false;
        };
        // Nobody settles down to work beside something that is hunting them.
        let hunted = game.character(who).is_some_and(|me| {
            game.creatures_at(me.place()).any(|creature| creature.being.kind.is_aggressive())
        });
        if hunted {
            return false;
        }
        match self {
            Self::Greedy => node.stock() >= MIN_WORKABLE_STOCK,
            Self::Careful => node.quality() == Quality::Pristine && node.stock() * 2 >= node.capacity(),
        }
    }
}

/// One step of a route: an exit to take, and what to do if the tide has
/// closed it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Leg {
    /// The exit's label.
    pub exit: &'static str,
    /// Wait for the exit to open (it is the only way back) instead of
    /// skipping this step.
    pub wait_if_closed: bool,
}

impl Leg {
    /// Take `exit`, or skip this step if it is closed.
    pub const fn go(exit: &'static str) -> Self {
        Self { exit, wait_if_closed: false }
    }

    /// Take `exit`, waiting for the tide if it is closed.
    pub const fn wait_for(exit: &'static str) -> Self {
        Self { exit, wait_if_closed: true }
    }
}

/// How one bot behaves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Plan {
    /// When it gathers.
    pub policy: Policy,
    /// Exits walked once, to get into position.
    pub approach: &'static [Leg],
    /// Exits walked round and round afterwards. Empty means stay put.
    pub circuit: &'static [Leg],
    /// Whether it challenges marked characters it meets, and picks up
    /// what they drop.
    pub warden: bool,
}

impl Plan {
    /// Walks the whole shoreline, gathering wherever `policy` allows.
    pub const fn roam(policy: Policy) -> Self {
        Self { policy, approach: &[], circuit: CIRCUIT, warden: false }
    }

    /// Walks to one place and stays, gathering whenever `policy` allows.
    pub const fn camp(policy: Policy, approach: &'static [Leg]) -> Self {
        Self { policy, approach, circuit: &[], warden: false }
    }

    /// The same plan, but challenging any marked character it meets.
    pub const fn as_warden(self) -> Self {
        Self { warden: true, ..self }
    }

    /// A small mixed cast for keeping a lone player company: mostly
    /// careful, with one greedy roamer among every four.
    pub fn company(size: usize) -> Vec<Self> {
        const CAST: [Plan; 4] = [
            Plan::roam(Policy::Careful),
            Plan::roam(Policy::Greedy),
            Plan::camp(Policy::Careful, DEPOSIT_ROUTES[3]),
            Plan::camp(Policy::Careful, DEPOSIT_ROUTES[8]),
        ];
        CAST.into_iter().cycle().take(size).collect()
    }

    /// A cast for a shared world that has to last: one greedy roamer
    /// among every eight, the rest careful, and wardens among them.
    ///
    /// The careful hold Restoration Rites over what the greedy strip,
    /// which is what keeps the zone standing (`eq-server/tests/week.rs`).
    pub fn settlement(size: usize) -> Vec<Self> {
        const CAST: [Plan; 8] = [
            Plan::roam(Policy::Careful).as_warden(),
            Plan::camp(Policy::Careful, DEPOSIT_ROUTES[3]),
            Plan::roam(Policy::Careful),
            Plan::roam(Policy::Greedy),
            Plan::roam(Policy::Careful).as_warden(),
            Plan::camp(Policy::Careful, DEPOSIT_ROUTES[8]),
            Plan::roam(Policy::Careful).as_warden(),
            Plan::camp(Policy::Careful, DEPOSIT_ROUTES[0]),
        ];
        CAST.into_iter().cycle().take(size).collect()
    }

    fn leg(&self, step: usize) -> Option<Leg> {
        match self.approach.get(step) {
            Some(leg) => Some(*leg),
            None if self.circuit.is_empty() => None,
            None => Some(self.circuit[(step - self.approach.len()) % self.circuit.len()]),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Bot {
    who: CharacterId,
    plan: Plan,
    step: usize,
    free_at: Tick,
    /// The life the bot was last seen living, to notice a rebirth.
    life: u32,
}

/// A set of bots and where each has got to in its plan.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Bots {
    bots: Vec<Bot>,
}

impl Bots {
    /// Puts each named character under a plan.
    pub fn new(crew: impl IntoIterator<Item = (CharacterId, Plan)>) -> Self {
        let bots = crew.into_iter().map(|(who, plan)| Bot { who, plan, step: 0, free_at: Tick::ZERO, life: 1 }).collect();
        Self { bots }
    }

    /// How many bots there are.
    pub fn len(&self) -> usize {
        self.bots.len()
    }

    /// Whether there are no bots.
    pub fn is_empty(&self) -> bool {
        self.bots.is_empty()
    }

    /// Runs the world to `until`, letting each bot act whenever it is
    /// free, soonest first. Returns everything that happened.
    pub fn run(&mut self, session: &mut Session<Game>, until: Tick) -> Result<Events<Game>, OutOfOrder> {
        let mut events = Vec::new();
        while let Some(index) = self.next_free_before(until) {
            self.take_turn(index, session, &mut events)?;
        }
        events.extend(session.advance_to(until)?);
        Ok(events)
    }

    /// Has every bot stand still until `now`, as if it had been resting.
    pub fn stand_still_until(&mut self, now: Tick) {
        for bot in &mut self.bots {
            bot.free_at = bot.free_at.max(now);
        }
    }

    fn next_free_before(&self, until: Tick) -> Option<usize> {
        (0..self.bots.len()).min_by_key(|&index| self.bots[index].free_at).filter(|&index| self.bots[index].free_at < until)
    }

    /// One bot's turn. In order of preference: sleep if worn out and
    /// somewhere it would sleep; a warden deals with a marked character
    /// or with spoils on the ground; a greedy bot robs a sleeper; any bot
    /// gathers if its policy says so; otherwise it walks the next leg of
    /// its plan.
    fn take_turn(&mut self, index: usize, session: &mut Session<Game>, events: &mut Events<Game>) -> Result<(), OutOfOrder> {
        let now = self.bots[index].free_at.max(session.now());
        // Bring the world up to the bot's turn first, so it decides on
        // what is there now and not on what was there when someone last
        // acted.
        events.extend(session.advance_to(now)?);

        let who = self.bots[index].who;
        let Some(me) = session.world().character(who) else { return Ok(()) };
        // A new life starts at camp, so the plan starts again too.
        if me.life() != self.bots[index].life {
            self.bots[index].life = me.life();
            self.bots[index].step = 0;
        }
        if let Some(death) = me.death().copied() {
            return self.haunt(index, &death, session, events, now);
        }
        let (bot, world) = (&self.bots[index], session.world());
        // The command to give, and the least time before the next turn.
        let choice = match sleep_duty(world, bot, now) {
            Sleep::Until(wake_at) => {
                self.bots[index].free_at = wake_at.max(now + PAUSE);
                return Ok(());
            }
            // Worn out: no stopping to gather on the way to shelter.
            Sleep::HeadHome => None,
            Sleep::Do(command) => Some((command, PAUSE)),
            Sleep::NotNow => beast_duty(world, bot)
                .or_else(|| warden_duty(world, bot))
                .or_else(|| thieving(world, bot, now))
                .or_else(|| rite_duty(world, bot, now))
                .map(|command| (command, PAUSE))
                .or_else(|| bot.plan.policy.wants_to_gather(world, who).then(|| (Command::gather(who), IDLE))),
        };
        let Some((command, pause)) = choice else {
            return self.walk(index, session, events, now);
        };
        events.extend(session.submit(now, command)?);
        self.bots[index].free_at = busy_until(session, who, now).max(now + pause);
        Ok(())
    }

    /// A dead bot makes for its body by the shortest way and returns to
    /// it as soon as it may. If it is too late, it gives the body up.
    fn haunt(&mut self, index: usize, death: &Death, session: &mut Session<Game>, events: &mut Events<Game>, now: Tick) -> Result<(), OutOfOrder> {
        let who = self.bots[index].who;
        let world = session.world();
        let here = world.character(who).map(Character::place);
        let command = if death.risen {
            // Too late: the body stands guard now. Start again.
            Some(Command::relinquish(who))
        } else if here == Some(death.body) {
            if now < death.ready_at {
                self.bots[index].free_at = death.ready_at;
                return Ok(());
            }
            Some(Command::return_to_body(who))
        } else {
            // Water is nothing to a spirit, so every exit will do.
            let step = here.and_then(|place| world.map().first_step(place, death.body, |_| true));
            step.map(|exit| Command::go(who, &exit.label))
        };
        match command {
            Some(command) => {
                events.extend(session.submit(now, command)?);
                self.bots[index].free_at = busy_until(session, who, now).max(now + PAUSE);
            }
            None => self.bots[index].free_at = now + IDLE,
        }
        Ok(())
    }

    /// Walks the next leg of the bot's plan, or stands idle if it has
    /// nowhere to go.
    fn walk(&mut self, index: usize, session: &mut Session<Game>, events: &mut Events<Game>, now: Tick) -> Result<(), OutOfOrder> {
        let bot = &mut self.bots[index];
        let Some(leg) = bot.plan.leg(bot.step) else {
            bot.free_at = now + IDLE;
            return Ok(());
        };
        let tried = session.submit(now, Command::go(bot.who, leg.exit))?;
        let closed = tried.iter().any(|stamped| {
            matches!(stamped.value, Event::Character { outcome: Outcome::ExitClosed { .. }, .. })
        });
        events.extend(tried);
        if closed && leg.wait_if_closed {
            bot.free_at = now + TIDE_WAIT;
        } else {
            // A step that led nowhere (somewhere flooded) is skipped.
            bot.step += 1;
            bot.free_at = busy_until(session, bot.who, now).max(now + PAUSE);
        }
        Ok(())
    }
}

/// When `who` will be free to act again, or `now` if they are unknown.
fn busy_until(session: &Session<Game>, who: CharacterId, now: Tick) -> Tick {
    session.world().character(who).map_or(now, |character| character.busy_until())
}

/// What a bot should do about sleep on this turn.
enum Sleep {
    /// Nothing: carry on with the day.
    NotNow,
    /// Lie down, or wake up.
    Do(Command),
    /// Already asleep: nothing to do before this tick.
    Until(Tick),
    /// Worn out but not somewhere it would sleep: keep walking.
    HeadHome,
}

/// A bot sleeps a full night once its rest is spent. Roamers walk on
/// until they reach a haven; campers sleep where they work, and take
/// their chances.
fn sleep_duty(game: &Game, bot: &Bot, now: Tick) -> Sleep {
    let Some(me) = game.character(bot.who) else { return Sleep::NotNow };
    if let Some(asleep_from) = me.asleep_from() {
        let wake_at = asleep_from + FULL_REST;
        return if now < wake_at { Sleep::Until(wake_at) } else { Sleep::Do(Command::wake(bot.who)) };
    }
    if me.rested() > 0 {
        return Sleep::NotNow;
    }
    let in_haven = game.place_of(bot.who).is_some_and(|place| place.safety == Safety::Haven);
    let camps = bot.plan.circuit.is_empty() && bot.step >= bot.plan.approach.len();
    if in_haven || camps {
        Sleep::Do(Command::rest(bot.who))
    } else {
        Sleep::HeadHome
    }
}

/// A greedy bot goes through the pack of any sleeper it finds unguarded.
fn thieving(game: &Game, bot: &Bot, now: Tick) -> Option<Command> {
    if bot.plan.policy != Policy::Greedy {
        return None;
    }
    let here = game.character(bot.who)?.place();
    let sleeper = game.occupants(here).iter().copied().find(|&other| game.can_rob(bot.who, other, now).is_ok())?;
    Some(Command::rob(bot.who, sleeper))
}

/// What a warden would do before anything else: challenge a marked
/// character standing with it, or pick up what lies on the ground.
fn warden_duty(game: &Game, bot: &Bot) -> Option<Command> {
    if !bot.plan.warden {
        return None;
    }
    let me = game.character(bot.who)?;
    let fit_to_fight = me.health() >= HEALTH_TO_FIGHT;
    let marked = game.occupants(me.place()).iter().copied().find(|&other| {
        game.character(other).is_some_and(|character| character.standing() == Standing::Marked)
            && game.can_attack(bot.who, other).is_ok()
    });
    if let (true, Some(target)) = (fit_to_fight, marked) {
        return Some(Command::attack(bot.who, target));
    }
    game.can_take(bot.who).then(|| Command::take(bot.who))
}

/// A careful bot mends what it would not work. It answers a rite
/// someone has begun, and begins one itself if there is anyone awake to
/// answer. The marked cannot, and the greedy do not.
fn rite_duty(game: &Game, bot: &Bot, now: Tick) -> Option<Command> {
    let me = game.character(bot.who)?;
    if bot.plan.policy != Policy::Careful || me.standing() == Standing::Marked {
        return None;
    }
    let deposit = game.place_of(bot.who)?.deposit?;
    if !deposit.needs_rite(now) {
        return None;
    }
    let answering = deposit.rite_open(now).is_some_and(|beginner| beginner != bot.who);
    let waiting = deposit.rite_open(now) == Some(bot.who);
    let company = game.occupants(me.place()).iter().any(|&other| {
        other != bot.who && game.character(other).is_some_and(|other| !other.is_spirit() && !other.is_lying_down())
    });
    (answering || (company && !waiting)).then(|| Command::rite(bot.who))
}

/// Any bot fit to fight goes for a creature that would go for it.
fn beast_duty(game: &Game, bot: &Bot) -> Option<Command> {
    let me = game.character(bot.who)?;
    if me.health() < HEALTH_TO_FIGHT {
        return None;
    }
    let beast = game.creatures_at(me.place()).find(|creature| creature.being.kind.is_aggressive())?;
    Some(Command::fight(bot.who, beast.id))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn happened(events: &Events<Game>, wanted: impl Fn(&Event) -> bool) -> bool {
        events.iter().any(|stamped| wanted(&stamped.value))
    }

    #[test]
    fn a_warden_kills_a_marked_bot_and_takes_what_it_drops() {
        let mut session = session(&["Ash", "Bryn"]);
        let ledge = DEPOSIT_ROUTES[0];
        let mut bots = Bots::new([
            (ASH, Plan::camp(Policy::Careful, ledge).as_warden()),
            (BRYN, Plan::camp(Policy::Greedy, ledge)),
        ]);
        let events = bots.run(&mut session, Tick::new(3_000)).unwrap();
        assert!(
            happened(&events, |event| matches!(event, Event::Character { who: BRYN, outcome: Outcome::Died { by: crate::Cause::Character(ASH), .. } })),
            "Bryn was never killed"
        );
        assert!(happened(&events, |event| matches!(event, Event::Character { who: ASH, outcome: Outcome::Took { .. } })));
        assert!(!happened(&events, |event| matches!(event, Event::Character { who: ASH, outcome: Outcome::Died { .. } })));
    }

    #[test]
    fn a_dead_bot_walks_back_to_its_body_and_returns_to_it() {
        let mut session = session(&["Ash", "Bryn"]);
        let ledge = DEPOSIT_ROUTES[0];
        let mut bots = Bots::new([
            (ASH, Plan::camp(Policy::Careful, ledge).as_warden()),
            (BRYN, Plan::camp(Policy::Greedy, ledge)),
        ]);
        // Long enough for one death and the walk back; a second death soon
        // after would hold the spirit too long to reach the body.
        let events = bots.run(&mut session, Tick::new(1_500)).unwrap();
        assert!(happened(&events, |event| matches!(event, Event::Character { who: BRYN, outcome: Outcome::Returned })));
        assert!(!happened(&events, |event| matches!(event, Event::Character { outcome: Outcome::Reborn { .. }, .. })));
        assert_eq!(session.world().character(BRYN).unwrap().life(), 1);
    }

    #[test]
    fn a_bot_whose_body_is_out_of_reach_in_time_is_reborn_and_starts_its_plan_again() {
        // The grotto is five legs from camp: too far to walk back in ten minutes.
        let mut session = session(&["Ash", "Bryn"]);
        let grotto = DEPOSIT_ROUTES[6];
        let mut bots = Bots::new([
            (ASH, Plan::camp(Policy::Careful, grotto).as_warden()),
            (BRYN, Plan::camp(Policy::Greedy, grotto)),
        ]);
        // Contested ground: the warden only strikes once Bryn is marked.
        let events = bots.run(&mut session, Tick::new(6_000)).unwrap();
        let died = happened(&events, |event| matches!(event, Event::Character { who: BRYN, outcome: Outcome::Died { .. } }));
        assert!(died, "Bryn was never killed");
        let bryn = session.world().character(BRYN).unwrap();
        // Too late to return: Bryn gave the body up and began again at camp.
        assert!(bryn.life() >= 2, "Bryn should have been reborn");
        assert!(happened(&events, |event| matches!(event, Event::Character { who: BRYN, outcome: Outcome::BodyRose })));
    }

    /// How long a bot stays up before it must sleep, with a little over.
    const A_LONG_DAY: u64 = crate::content::RESTED_LASTS.as_ticks() + 3_600;

    fn woke(events: &Events<Game>, who: CharacterId) -> Option<Outcome> {
        events.iter().find_map(|stamped| match &stamped.value {
            Event::Character { who: woken, outcome: outcome @ Outcome::Woke { .. } } if *woken == who => Some(outcome.clone()),
            _ => None,
        })
    }

    #[test]
    fn a_worn_out_roamer_walks_home_sleeps_a_night_and_wakes_rested() {
        let mut session = session(&["Ash"]);
        let mut bots = Bots::new([(ASH, Plan::roam(Policy::Careful))]);
        let mut events = bots.run(&mut session, Tick::new(A_LONG_DAY)).unwrap();

        let ash = session.world().character(ASH).unwrap();
        assert!(ash.is_asleep(session.now()), "Ash should be asleep by now");
        assert_eq!(session.world().place_of(ASH).unwrap().name, "Tidewatch Camp");

        events.extend(bots.run(&mut session, Tick::new(A_LONG_DAY + FULL_REST.as_ticks())).unwrap());
        let Some(Outcome::Woke { slept, rested, wasted }) = woke(&events, ASH) else { panic!("Ash never woke") };
        assert_eq!((slept, rested, wasted), (FULL_REST, 100, 0));
    }

    #[test]
    fn a_camper_sleeps_where_it_works() {
        let mut session = session(&["Ash"]);
        let mut bots = Bots::new([(ASH, Plan::camp(Policy::Careful, DEPOSIT_ROUTES[0]))]);
        bots.run(&mut session, Tick::new(A_LONG_DAY)).unwrap();
        assert!(session.world().character(ASH).unwrap().is_asleep(session.now()));
        assert_eq!(session.world().place_of(ASH).unwrap().name, "Ore Ledge");
    }

    #[test]
    fn a_greedy_bot_robs_a_sleeper_in_the_wilds_and_is_marked_for_it() {
        // Ash is played by hand: walk to the ledge, gather, lie down.
        let mut session = session(&["Ash", "Bryn"]);
        session.submit(Tick::new(0), Command::go(ASH, "north")).unwrap();
        session.submit(Tick::new(30), Command::gather(ASH)).unwrap();
        session.submit(Tick::new(35), Command::rest(ASH)).unwrap();

        let mut bots = Bots::new([(BRYN, Plan::camp(Policy::Greedy, DEPOSIT_ROUTES[0]))]);
        let events = bots.run(&mut session, Tick::new(200)).unwrap();

        let robbed = Event::Character { who: ASH, outcome: Outcome::Robbed };
        assert!(events.iter().any(|stamped| stamped.value == robbed), "Ash was never robbed");
        let (ash, bryn) = (session.world().character(ASH).unwrap(), session.world().character(BRYN).unwrap());
        assert_eq!(ash.carried_total(), 0);
        assert!(ash.is_asleep(session.now()), "being robbed should not wake you");
        assert_eq!(bryn.standing(), Standing::Marked);
    }

    #[test]
    fn nobody_is_robbed_asleep_at_camp() {
        let mut session = session(&["Ash", "Bryn"]);
        session.submit(Tick::new(0), Command::go(ASH, "north")).unwrap();
        session.submit(Tick::new(30), Command::gather(ASH)).unwrap();
        session.submit(Tick::new(35), Command::go(ASH, "south")).unwrap();
        session.submit(Tick::new(65), Command::rest(ASH)).unwrap();

        // Bryn never leaves camp, and would rob Ash if it could.
        let mut bots = Bots::new([(BRYN, Plan::camp(Policy::Greedy, &[]))]);
        bots.run(&mut session, Tick::new(1_000)).unwrap();
        assert!(session.world().character(ASH).unwrap().carried_total() > 0);
    }

    #[test]
    fn a_waiting_bot_sees_the_world_change_around_it() {
        // A careful camper alone: once the ledge is no longer pristine it
        // must wait, with nobody else acting, and notice when it heals.
        let mut session = session(&["Ash"]);
        let mut bots = Bots::new([(ASH, Plan::camp(Policy::Careful, DEPOSIT_ROUTES[0]))]);
        bots.run(&mut session, Tick::new(1_000)).unwrap();
        let early = session.world().character(ASH).unwrap().carried_total();
        bots.run(&mut session, Tick::new(10_000)).unwrap();
        let late = session.world().character(ASH).unwrap().carried_total();
        assert!(late > early * 3, "it stopped noticing: {early} then {late}");
    }

    #[test]
    fn bots_that_are_not_wardens_never_fight() {
        let mut session = session(&["Ash", "Bryn"]);
        let ledge = DEPOSIT_ROUTES[0];
        let mut bots =
            Bots::new([(ASH, Plan::camp(Policy::Careful, ledge)), (BRYN, Plan::camp(Policy::Greedy, ledge))]);
        let events = bots.run(&mut session, Tick::new(3_000)).unwrap();
        assert!(!events.iter().any(|stamped| {
            matches!(stamped.value, Event::Character { outcome: Outcome::Struck { .. }, .. })
        }));
    }

    const ASH: CharacterId = CharacterId::new(0);
    const BRYN: CharacterId = CharacterId::new(1);

    fn session(names: &[&str]) -> Session<Game> {
        Session::new(Game::new(1, names))
    }

    #[test]
    fn a_camper_walks_to_its_place_and_stays() {
        let mut session = session(&["Ash"]);
        let mut bots = Bots::new([(ASH, Plan::camp(Policy::Greedy, DEPOSIT_ROUTES[1]))]);
        bots.run(&mut session, Tick::new(600)).unwrap();
        assert_eq!(session.world().place_of(ASH).unwrap().name, "High Shelf");
        assert!(session.world().character(ASH).unwrap().carried_total() > 0);
    }

    #[test]
    fn a_roamer_gets_round_the_whole_shoreline() {
        let mut session = session(&["Ash"]);
        let mut bots = Bots::new([(ASH, Plan::roam(Policy::Careful))]);
        let events = bots.run(&mut session, Tick::new(6_000)).unwrap();
        let mut visited: Vec<u64> = events
            .iter()
            .filter_map(|stamped| match stamped.value {
                Event::Character { outcome: Outcome::Moved { to, .. }, .. } => Some(to.get()),
                _ => None,
            })
            .collect();
        visited.sort_unstable();
        visited.dedup();
        assert_eq!(visited.len(), session.world().map().len(), "visited {visited:?}");
    }

    #[test]
    fn careful_bots_stay_clean_and_greedy_ones_get_marked() {
        let mut session = session(&["Ash", "Bryn"]);
        let mut bots = Bots::new([(ASH, Plan::roam(Policy::Careful)), (BRYN, Plan::roam(Policy::Greedy))]);
        bots.run(&mut session, Tick::new(3_000)).unwrap();
        assert_eq!(session.world().character(ASH).unwrap().taint(), 0);
        assert!(session.world().character(BRYN).unwrap().taint() >= 50);
    }

    #[test]
    fn standing_still_stops_the_bots_without_stopping_the_world() {
        let mut session = session(&["Ash"]);
        let mut bots = Bots::new([(ASH, Plan::camp(Policy::Greedy, DEPOSIT_ROUTES[0]))]);
        bots.run(&mut session, Tick::new(300)).unwrap();
        let commands = session.log().len();

        session.advance_to(Tick::new(900)).unwrap();
        bots.stand_still_until(session.now());
        assert_eq!(session.log().len(), commands);
        bots.run(&mut session, Tick::new(910)).unwrap();
        assert!(session.log().len() > commands);
    }

    #[test]
    fn the_company_is_mostly_careful() {
        let cast = Plan::company(8);
        let greedy = cast.iter().filter(|plan| plan.policy == Policy::Greedy).count();
        assert_eq!((cast.len(), greedy), (8, 2));
        assert!(Plan::company(0).is_empty());
        assert!(Bots::default().is_empty());
        assert_eq!(Bots::new([(ASH, cast[0])]).len(), 1);
    }
}
