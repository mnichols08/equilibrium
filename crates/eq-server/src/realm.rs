//! One world and the players in it, with no network in sight.
//!
//! A [`Realm`] owns the session, saves every command as it is applied,
//! and knows who is connected. Whoever drives it (the WebSocket server,
//! a test) calls three things: [`Realm::enter`] when someone arrives,
//! [`Realm::line`] when they type, and [`Realm::tick`] once per tick of
//! world time. Each hands back what to tell whom.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use eq_clock::{Duration, Tick};
use eq_events::{Events, Session};
use eq_game::bots::{Bots, Plan};
use eq_game::content::DISCONNECT_GRACE;
use eq_game::seat::{interpret, Request};
use eq_game::{narrate, Action, CharacterId, Command, Event, Game, Outcome};
use eq_scene::Scene;
use eq_store::{LogFile, StoreError};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Names for the bots that share a realm, in order.
const BOT_NAMES: [&str; 24] = [
    "Ash", "Bryn", "Cole", "Dara", "Eli", "Fen", "Gil", "Hale", "Ivo", "Jun", "Kit", "Lark", "Mab", "Nell", "Orrin",
    "Pell", "Quin", "Rook", "Sable", "Tam", "Una", "Vey", "Wren", "Yarrow",
];

/// How often the realm writes down what tick it has reached. Ticks in
/// which nobody acts are not in the command log, so without this a
/// restart would wind the clock back to the last command.
const CLOCK_SAVED_EVERY: u64 = 60;

/// How often a player who asked for scenes is sent a fresh one even
/// though nothing was said to them: the clock and the tide move, and
/// others come and go out of earshot.
const SCENE_EVERY: u64 = 5;

/// The shortest and longest a character's name can be.
const NAME_LENGTH: std::ops::RangeInclusive<usize> = 3..=16;

/// How a realm is set up.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    /// The world's random seed. Ignored if the realm already exists on disk.
    pub seed: u64,
    /// How many bots share the world. Ignored if the realm already exists.
    pub bots: usize,
    /// How many players may found a territory of their own. Ignored if
    /// the realm already exists.
    pub founders: usize,
    /// Where the realm is saved, or `None` to keep it in memory only.
    pub data: Option<PathBuf>,
}

/// What never changes about a realm, saved beside its log.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Founding {
    seed: u64,
    bots: usize,
    /// Left out of realms saved before there were founders: none.
    #[serde(default)]
    founders: usize,
}

/// Why a realm could not be opened.
#[derive(Debug)]
pub enum RealmError {
    /// The saved log could not be read or written.
    Store(StoreError),
    /// Another saved file could not be read or written.
    Io(std::io::Error),
    /// A saved file was not what it should be.
    Corrupt(String),
}

impl std::fmt::Display for RealmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Store(error) => write!(f, "{error}"),
            Self::Io(error) => write!(f, "the realm's files could not be read or written: {error}"),
            Self::Corrupt(problem) => write!(f, "the realm's files are damaged: {problem}"),
        }
    }
}

impl std::error::Error for RealmError {}

impl From<StoreError> for RealmError {
    fn from(error: StoreError) -> Self {
        Self::Store(error)
    }
}

impl From<std::io::Error> for RealmError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

/// Why someone was not let in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The name is not 3 to 16 letters.
    BadName,
    /// The name belongs to someone else: the passphrase did not match.
    WrongKey,
    /// The name belongs to one of the realm's bots.
    Taken,
}

impl Refusal {
    /// The refusal in words fit to show the player.
    pub const fn reason(self) -> &'static str {
        match self {
            Self::BadName => "A name is 3 to 16 letters, with nothing else in it.",
            Self::WrongKey => "That name is already someone's, and that is not their passphrase.",
            Self::Taken => "That name is taken.",
        }
    }
}

/// Someone who was let in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entered {
    /// Their character.
    pub who: CharacterId,
    /// The character's name as the world has it.
    pub name: String,
    /// Whether this made the character.
    pub new: bool,
    /// What to tell everyone about it, the newcomer included.
    pub deliveries: Vec<Delivery>,
}

/// Something for one connected player.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Delivery {
    /// Whose player to tell.
    pub to: CharacterId,
    /// What to tell them, in order. May be empty if only the scene changed.
    pub lines: Vec<String>,
    /// What they can see now, if their client asked for scenes.
    pub scene: Option<serde_json::Value>,
}

/// One world, saved as it goes, and the players connected to it.
#[derive(Debug)]
pub struct Realm {
    founding: Founding,
    session: Session<Game>,
    store: Option<LogFile<Command>>,
    /// How many of the session's commands are already in the store.
    saved: usize,
    /// A digest of each player's passphrase, by lower-cased name. The
    /// passphrases themselves are never kept.
    keys: BTreeMap<String, String>,
    keys_path: Option<PathBuf>,
    clock_path: Option<PathBuf>,
    online: BTreeSet<CharacterId>,
    /// The connected players whose clients draw scenes.
    watching: BTreeSet<CharacterId>,
    /// Players who dropped away, and when each will lie down.
    leaving: Vec<(CharacterId, Tick)>,
    bots: Bots,
}

impl Realm {
    /// Opens the realm saved in `config.data`, or founds a new one.
    ///
    /// An existing realm is rebuilt by replaying its saved commands, so
    /// it comes back exactly as it was left, at the tick of its last
    /// command.
    pub fn open(config: &Config) -> Result<Self, RealmError> {
        let Some(data) = &config.data else {
            let founding = Founding { seed: config.seed, bots: config.bots, founders: config.founders };
            return Ok(Self::found(founding, None, None, BTreeMap::new()));
        };
        fs::create_dir_all(data)?;
        let founding = match read_json::<Founding>(&data.join("realm.json"))? {
            Some(founding) => founding,
            None => {
                let founding = Founding { seed: config.seed, bots: config.bots, founders: config.founders };
                write_json(&data.join("realm.json"), &founding)?;
                founding
            }
        };
        let keys_path = data.join("keys.json");
        let keys = read_json(&keys_path)?.unwrap_or_default();
        let (store, commands) = LogFile::open(&data.join("commands.log"))?;

        let clock_path = data.join("clock.json");
        let clock = read_json::<u64>(&clock_path)?.unwrap_or(0);
        let mut realm = Self::found(founding, Some(store), Some(keys_path), keys);
        realm.clock_path = Some(clock_path);
        if !commands.is_empty() {
            realm.session = Session::new(fresh_world(&founding));
            for (tick, command) in commands {
                realm.session.submit(Tick::new(tick), command).map_err(|_| {
                    RealmError::Corrupt(format!("a command at tick {tick} is out of order in commands.log"))
                })?;
            }
            realm.saved = realm.session.log().len();
        }
        // Quiet ticks since the last command are not in the log.
        if Tick::new(clock) > realm.now() {
            realm.session.advance_to(Tick::new(clock)).expect("the saved clock is ahead of the world");
        }
        realm.save_new_commands()?;
        Ok(realm)
    }

    /// A new realm: the world as it begins, with its bots at their rounds.
    fn found(founding: Founding, store: Option<LogFile<Command>>, keys_path: Option<PathBuf>, keys: BTreeMap<String, String>) -> Self {
        let session = Session::new(fresh_world(&founding));
        let crew: Vec<(CharacterId, Plan)> =
            Plan::settlement(founding.bots).into_iter().enumerate().map(|(index, plan)| (CharacterId::new(index as u64), plan)).collect();
        Self {
            founding,
            session,
            store,
            saved: 0,
            keys,
            keys_path,
            clock_path: None,
            online: BTreeSet::new(),
            watching: BTreeSet::new(),
            leaving: Vec::new(),
            bots: Bots::new(crew),
        }
    }

    /// The seed the world was founded with.
    pub const fn seed(&self) -> u64 {
        self.founding.seed
    }

    /// Writes down everything not yet saved, clock included. Call it
    /// before shutting down; a realm that is merely dropped loses at
    /// most the last minute of quiet ticks.
    pub fn close(&mut self) {
        self.persist();
        self.save_clock();
    }

    /// The world as it stands.
    pub const fn game(&self) -> &Game {
        self.session.world()
    }

    /// The current tick.
    pub const fn now(&self) -> Tick {
        self.session.now()
    }

    /// Whether `who`'s player is connected.
    pub fn is_online(&self, who: CharacterId) -> bool {
        self.online.contains(&who)
    }

    /// Lets a player in. A new name makes a new character and sets its
    /// passphrase; a known name must come with the same passphrase.
    /// With `scenes`, everything they are told comes with what they can
    /// see, for a client that draws.
    pub fn enter(&mut self, name: &str, key: &str, scenes: bool) -> Result<Entered, Refusal> {
        let name = name.trim();
        if !NAME_LENGTH.contains(&name.len()) || !name.chars().all(|letter| letter.is_ascii_alphabetic()) {
            return Err(Refusal::BadName);
        }
        let known = self.game().characters().find(|(_, character)| character.name().eq_ignore_ascii_case(name)).map(|(id, _)| id);
        let handle = name.to_ascii_lowercase();
        let key = digest(&handle, key);
        let (who, new, events) = match (known, self.keys.get(&handle)) {
            (Some(who), Some(set)) if *set == key => (who, false, Vec::new()),
            (Some(_), Some(_)) => return Err(Refusal::WrongKey),
            // A character nobody holds the key to is one of the bots.
            (Some(_), None) => return Err(Refusal::Taken),
            (None, _) => {
                let who = self.game().next_id();
                let events = self.submit(Command::join(who, name));
                self.keys.insert(handle, key);
                self.save_keys();
                (who, true, events)
            }
        };
        self.online.insert(who);
        if scenes {
            self.watching.insert(who);
        } else {
            self.watching.remove(&who);
        }
        self.leaving.retain(|(leaver, _)| *leaver != who);

        let mut deliveries = self.told(&events);
        let mut greeting = vec![narrate::place(self.game(), who, self.now())];
        let character = self.game().character(who);
        if character.is_some_and(|character| character.is_lying_down() || character.is_dormant()) {
            greeting.push("You are asleep. Type \"wake\" to get up.".into());
        }
        push_lines(&mut deliveries, who, greeting);
        self.attach_scenes(&mut deliveries);
        let name = character.map_or(name, |character| character.name()).to_owned();
        Ok(Entered { who, name, new, deliveries })
    }

    /// Notes that `who`'s player has dropped away. The character stays
    /// on their feet for a short grace, then lies down where they stand.
    pub fn leave(&mut self, who: CharacterId) {
        self.watching.remove(&who);
        if self.online.remove(&who) {
            self.leaving.push((who, self.now() + DISCONNECT_GRACE));
        }
    }

    /// Handles one line typed by `who`'s player.
    pub fn line(&mut self, who: CharacterId, text: &str) -> Vec<Delivery> {
        let lines = |lines: Vec<String>| vec![Delivery { to: who, lines, scene: None }];
        match interpret(self.game(), who, self.now(), text) {
            Request::Act(action) => self.act(who, action),
            // Whatever span was asked for, sleep here lasts until they wake.
            Request::Sleep(_) => self.act(who, Action::Rest),
            Request::Wait(_) => lines(vec!["Time passes by itself here. There is no need to wait for it.".into()]),
            Request::Say(said) => lines(said),
            Request::Quit => lines(vec!["Close the connection to leave. Your character stays in the world.".into()]),
        }
    }

    /// Runs one tick of world time: the bots act, and anyone who
    /// dropped away long enough ago lies down.
    pub fn tick(&mut self) -> Vec<Delivery> {
        let next = self.now() + Duration::ticks(1);
        let mut events = self.bots.run(&mut self.session, next).expect("the realm's clock only runs forward");
        events.extend(self.settle_the_departed());
        self.persist();
        if self.now().get().is_multiple_of(CLOCK_SAVED_EVERY) {
            self.save_clock();
        }
        let mut deliveries = self.told(&events);
        if self.now().get().is_multiple_of(SCENE_EVERY) {
            for &watcher in &self.watching {
                push_lines(&mut deliveries, watcher, Vec::new());
            }
        }
        self.attach_scenes(&mut deliveries);
        deliveries
    }

    /// Has `who` act now, and tells everyone who would notice.
    fn act(&mut self, who: CharacterId, action: Action) -> Vec<Delivery> {
        let events = self.submit(Command { who, action });
        let moved = events.iter().any(|stamped| {
            matches!(&stamped.value, Event::Character { who: mover, outcome: Outcome::Moved { .. } } if *mover == who)
        });
        let mut deliveries = self.told(&events);
        if moved {
            push_lines(&mut deliveries, who, vec![narrate::place(self.game(), who, self.now())]);
        }
        // Whoever acted sees the result even if nothing was said of it.
        push_lines(&mut deliveries, who, Vec::new());
        self.attach_scenes(&mut deliveries);
        deliveries
    }

    /// Lays down everyone whose grace has run out, unless they are back,
    /// already down, or dead.
    fn settle_the_departed(&mut self) -> Events<Game> {
        let now = self.now();
        let due: Vec<CharacterId> = self.leaving.iter().filter(|(_, at)| *at <= now).map(|(who, _)| *who).collect();
        self.leaving.retain(|(_, at)| *at > now);
        let mut events = Vec::new();
        for who in due {
            let up = self.game().character(who).is_some_and(|character| !character.is_lying_down() && !character.is_spirit());
            if up && !self.is_online(who) {
                events.extend(self.session.submit(now, Command::rest(who)).expect("now is never in the past"));
            }
        }
        events
    }

    /// Applies a command at the present tick and saves it.
    fn submit(&mut self, command: Command) -> Events<Game> {
        let events = self.session.submit(self.now(), command).expect("now is never in the past");
        self.persist();
        events
    }

    /// Saves what has not been saved yet. A realm that cannot save must
    /// not carry on as if it had: the world would come back different.
    fn persist(&mut self) {
        self.save_new_commands().expect("the realm could not save its commands");
    }

    fn save_new_commands(&mut self) -> Result<(), StoreError> {
        let Some(store) = &mut self.store else { return Ok(()) };
        let entries = self.session.log().entries();
        for entry in &entries[self.saved..] {
            store.append(entry.tick.get(), &entry.value)?;
        }
        self.saved = entries.len();
        Ok(())
    }

    fn save_clock(&self) {
        if let Some(path) = &self.clock_path {
            write_json(path, &self.now().get()).expect("the realm could not save its clock");
        }
    }

    fn save_keys(&self) {
        if let Some(path) = &self.keys_path {
            write_json(path, &self.keys).expect("the realm could not save its players' keys");
        }
    }

    /// Gives every delivery to a player who draws the scene as it now is.
    fn attach_scenes(&self, deliveries: &mut [Delivery]) {
        for delivery in deliveries.iter_mut().filter(|delivery| self.watching.contains(&delivery.to)) {
            delivery.scene = Scene::of(self.game(), delivery.to, self.now())
                .map(|scene| serde_json::to_value(scene).expect("a scene holds only plain data"));
        }
    }

    /// What each connected player would notice among `events`, in order.
    fn told(&self, events: &Events<Game>) -> Vec<Delivery> {
        let mut deliveries = Vec::new();
        for stamped in events {
            for viewer in self.game().audience(&stamped.value) {
                if !self.is_online(viewer) {
                    continue;
                }
                if let Some(line) = narrate::event(&stamped.value, self.game(), viewer) {
                    push_lines(&mut deliveries, viewer, vec![line]);
                }
            }
        }
        deliveries
    }
}

/// What is kept in place of a passphrase: a SHA-256 digest of it,
/// salted with the name it belongs to, as hex.
///
/// This keeps passphrases out of the realm's files. It is not a slow
/// password hash: someone with the files could still try guesses fast.
fn digest(handle: &str, key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(handle.as_bytes());
    hasher.update([0]);
    hasher.update(key.as_bytes());
    hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect()
}

/// The world a realm begins as: the Rocky Shoreline with the bots
/// already settled on it, and room for the founders to come.
fn fresh_world(founding: &Founding) -> Game {
    let bots: Vec<String> = (0..founding.bots).map(bot_name).collect();
    let names: Vec<&str> = bots.iter().map(String::as_str).collect();
    Game::new_world(founding.seed, founding.founders, &names)
}

/// Adds `lines` to what `to` will be told.
fn push_lines(deliveries: &mut Vec<Delivery>, to: CharacterId, lines: Vec<String>) {
    match deliveries.iter_mut().find(|delivery| delivery.to == to) {
        Some(delivery) => delivery.lines.extend(lines),
        None => deliveries.push(Delivery { to, lines, scene: None }),
    }
}

/// The name of the `index`th bot.
fn bot_name(index: usize) -> String {
    BOT_NAMES.get(index).map_or_else(|| format!("Stranger{}", roman(index + 1 - BOT_NAMES.len())), |name| (*name).to_owned())
}

/// `n` as letters, so that bot names stay all letters: 1 is A, 27 is AA.
fn roman(mut n: usize) -> String {
    let mut letters = Vec::new();
    while n > 0 {
        n -= 1;
        letters.push(b'A' + (n % 26) as u8);
        n /= 26;
    }
    letters.reverse();
    String::from_utf8(letters).expect("only letters were pushed")
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<Option<T>, RealmError> {
    if !path.exists() {
        return Ok(None);
    }
    let text = fs::read_to_string(path)?;
    serde_json::from_str(&text).map(Some).map_err(|error| RealmError::Corrupt(format!("{}: {error}", path.display())))
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), std::io::Error> {
    fs::write(path, serde_json::to_string_pretty(value)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_passphrase_is_kept_only_as_a_digest_tied_to_its_name() {
        let kept = digest("ada", "hunter");
        assert_eq!(kept.len(), 64);
        assert!(!kept.contains("hunter"));
        assert_eq!(kept, digest("ada", "hunter"));
        assert_ne!(kept, digest("ben", "hunter"), "the same passphrase under another name looks different");
        assert_ne!(digest("ab", "c"), digest("a", "bc"), "name and passphrase cannot run together");
    }

    #[test]
    fn bots_beyond_the_named_ones_still_get_names_of_letters_only() {
        assert_eq!(bot_name(0), "Ash");
        assert_eq!(bot_name(BOT_NAMES.len()), "StrangerA");
        assert_eq!(bot_name(BOT_NAMES.len() + 26), "StrangerAA");
    }
}
