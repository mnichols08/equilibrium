//! Text to intent.
//!
//! The parser knows words, not the world: it does not check that an exit
//! exists or that there is anything to gather. The rules decide that.

use alloc::string::String;

use eq_clock::Duration;

use crate::board::{ContractId, Lot};
use crate::command::Action;
use crate::pact::Dealing;
use crate::types::Resource;

/// How long `wait` lasts when no time is given.
pub const DEFAULT_WAIT: Duration = Duration::minutes(1);

/// The longest single `wait`.
pub const MAX_WAIT: Duration = Duration::days(1);

/// How long `sleep` lasts when no time is given.
pub const DEFAULT_SLEEP: Duration = Duration::hours(8);

/// The longest single `sleep`.
pub const MAX_SLEEP: Duration = Duration::days(30);

/// What a line of input asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Input {
    /// Have the player's character act.
    Do(Action),
    /// Attack someone by name. The parser does not know who is here, so
    /// whoever holds the game turns the name into a character.
    Attack(String),
    /// Let time pass.
    Wait(Duration),
    /// Lie down, sleep this long, and wake.
    Sleep(Duration),
    /// Go through a sleeper's pack, by name.
    Rob(String),
    /// Bury a body, by the name of whose it is.
    Bury(String),
    /// Speak a forgotten spirit's name at the shrine.
    Name(String),
    /// Tend a sleeper, by name.
    Tend(String),
    /// Put a price on someone's head, by name.
    Bounty(String, Lot),
    /// Read the Contract Board.
    Board,
    /// Describe the current place.
    Look,
    /// Examine the deposit here and the zone's health.
    Inspect,
    /// Show skills and attributes.
    Stats,
    /// Show what is carried.
    Pack,
    /// List the commands.
    Help,
    /// Stop playing.
    Quit,
    /// A blank line.
    Empty,
    /// Not understood. Holds the reason, ready to show.
    Unknown(String),
}

/// Reads one line of input.
pub fn parse(line: &str) -> Input {
    // Names a player gives keep their own capitals; everything else is
    // read without regard to them.
    let given = line.trim().split_once(char::is_whitespace).map_or("", |(_, rest)| rest.trim());
    let line = line.trim().to_lowercase();
    let mut words = line.split_whitespace();
    let Some(verb) = words.next() else { return Input::Empty };
    let rest = words.next();

    match verb {
        "look" | "l" => Input::Look,
        "inspect" | "examine" | "x" => Input::Inspect,
        "stats" | "skills" | "st" => Input::Stats,
        "pack" | "inventory" | "inv" | "i" => Input::Pack,
        "help" | "?" => Input::Help,
        "quit" | "exit" | "q" => Input::Quit,
        "gather" | "g" | "mine" | "forage" | "harvest" => Input::Do(Action::Gather),
        "take" | "loot" | "grab" => Input::Do(Action::Take),
        "attack" | "fight" | "strike" | "hit" => match rest {
            Some(name) => Input::Attack(name.into()),
            None => Input::Unknown("Attack whom? Try \"attack bryn\".".into()),
        },
        "wait" | "z" => match time(rest, DEFAULT_WAIT, MAX_WAIT) {
            Some(span) => Input::Wait(span),
            None => Input::Unknown("Wait how long? Try \"wait 30\", \"wait 10m\" or \"wait 2h\".".into()),
        },
        "sleep" | "rest" => match time(rest, DEFAULT_SLEEP, MAX_SLEEP) {
            Some(span) => Input::Sleep(span),
            None => Input::Unknown("Sleep how long? Try \"sleep\", \"sleep 8h\" or \"sleep 4d\".".into()),
        },
        "wake" => Input::Do(Action::Wake),
        "return" => Input::Do(Action::Return),
        "relinquish" | "rebirth" => Input::Do(Action::Relinquish),
        "seek" | "remember" => Input::Do(Action::Seek),
        "name" => match rest {
            Some(name) => Input::Name(name.into()),
            None => Input::Unknown("Name whom? Try \"name bryn\".".into()),
        },
        "call" | "house" | "invite" | "ally" | "war" | "truce" if given.is_empty() => {
            Input::Unknown(alloc::format!("{verb} what? Give a name: \"{verb} Saltmarch\"."))
        }
        "call" => Input::Do(Action::NameLand(given.into())),
        "house" => Input::Do(Action::NameHouse(given.into())),
        "invite" => Input::Do(Action::Invite(given.into())),
        "ally" => Input::Do(Action::Deal { with: given.into(), dealing: Dealing::Ally }),
        "war" => Input::Do(Action::Deal { with: given.into(), dealing: Dealing::War }),
        "truce" => Input::Do(Action::Deal { with: given.into(), dealing: Dealing::Truce }),
        "raise" | "build" => Input::Do(Action::Raise),
        "rite" | "mend" => Input::Do(Action::Rite),
        "tend" | "vigil" => match rest {
            Some(name) => Input::Tend(name.into()),
            None => Input::Unknown("Tend whom? Try \"tend bryn\".".into()),
        },
        "board" | "contracts" => Input::Board,
        "post" | "offer" => match offer(rest, words) {
            Some((give, want)) => Input::Do(Action::Post { give, want }),
            None => Input::Unknown("Post what? Try \"post 10 ore for 30 kelp\".".into()),
        },
        "accept" | "withdraw" => match rest.and_then(|number| number.parse().ok()).map(ContractId::new) {
            Some(id) if verb == "accept" => Input::Do(Action::Accept(id)),
            Some(id) => Input::Do(Action::Withdraw(id)),
            None => Input::Unknown(alloc::format!("Which contract? Try \"{verb} 3\". \"board\" lists them.")),
        },
        "bounty" => match (rest, words.next(), words.next()) {
            (Some(name), Some(amount), Some(resource)) if lot(amount, resource).is_some_and(|reward| reward.amount > 0) => {
                Input::Bounty(name.into(), lot(amount, resource).expect("checked above"))
            }
            _ => Input::Unknown("A bounty on whom, of what? Try \"bounty bryn 30 ore\".".into()),
        },
        "collect" => Input::Do(Action::Collect),
        "bury" => match rest {
            Some(name) => Input::Bury(name.into()),
            None => Input::Unknown("Bury whom? Try \"bury bryn\".".into()),
        },
        "rob" | "steal" => match rest {
            Some(name) => Input::Rob(name.into()),
            None => Input::Unknown("Rob whom? Try \"rob bryn\".".into()),
        },
        "go" | "walk" | "enter" => match rest {
            Some(label) => go(label),
            None => Input::Unknown("Go where? Try \"go north\".".into()),
        },
        "north" | "south" | "east" | "west" | "up" | "down" | "cave" | "out" | "beyond" => go(verb),
        "n" | "s" | "e" | "w" | "u" | "d" => go(verb),
        _ => Input::Unknown(alloc::format!("I don't know how to \"{verb}\". Type \"help\" for commands.")),
    }
}

/// Reads "10 ore for 30 kelp": the amount is `first`, the rest follows.
fn offer<'a>(first: Option<&str>, mut rest: impl Iterator<Item = &'a str>) -> Option<(Lot, Lot)> {
    let give = lot(first?, rest.next()?)?;
    if rest.next()? != "for" {
        return None;
    }
    let want = lot(rest.next()?, rest.next()?)?;
    Some((give, want))
}

/// An amount and what it is an amount of.
fn lot(amount: &str, resource: &str) -> Option<Lot> {
    let resource = match resource {
        "ore" => Resource::Ore,
        "driftwood" | "wood" => Resource::Driftwood,
        "kelp" => Resource::Kelp,
        "tide-glass" | "tideglass" | "glass" => Resource::Tideglass,
        _ => return None,
    };
    Some(Lot { resource, amount: amount.parse().ok()? })
}

fn go(label: &str) -> Input {
    let label = match label {
        "n" => "north",
        "s" => "south",
        "e" => "east",
        "w" => "west",
        "u" => "up",
        "d" => "down",
        other => other,
    };
    Input::Do(Action::Go(label.into()))
}

/// Reads a length of time: a bare number of seconds, or a number ending
/// in `s`, `m`, `h` or `d`. Nothing at all means `default`; anything
/// longer than `most` is cut to `most`. Returns `None` for nonsense.
fn time(amount: Option<&str>, default: Duration, most: Duration) -> Option<Duration> {
    let Some(amount) = amount else { return Some(default) };
    let (digits, unit_seconds) = match amount.as_bytes().last() {
        Some(b'd') => (&amount[..amount.len() - 1], 86_400),
        Some(b'h') => (&amount[..amount.len() - 1], 3_600),
        Some(b'm') => (&amount[..amount.len() - 1], 60),
        Some(b's') => (&amount[..amount.len() - 1], 1),
        _ => (amount, 1),
    };
    let count = digits.parse::<u64>().ok().filter(|&count| count > 0)?;
    Some(Duration::seconds(count.saturating_mul(unit_seconds).min(most.as_ticks())))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn go_to(label: &str) -> Input {
        Input::Do(Action::Go(label.into()))
    }

    #[test]
    fn movement_in_all_its_spellings() {
        assert_eq!(parse("go north"), go_to("north"));
        assert_eq!(parse("  N  "), go_to("north"));
        assert_eq!(parse("west"), go_to("west"));
        assert_eq!(parse("enter cave"), go_to("cave"));
        assert_eq!(parse("out"), go_to("out"));
        assert_eq!(parse("d"), go_to("down"));
        assert_eq!(parse("go up"), go_to("up"));
        assert_eq!(parse("go rope-bridge"), go_to("rope-bridge"));
        assert!(matches!(parse("go"), Input::Unknown(_)));
    }

    #[test]
    fn gathering_synonyms() {
        for word in ["gather", "g", "mine", "forage", "harvest", "GATHER"] {
            assert_eq!(parse(word), Input::Do(Action::Gather), "{word}");
        }
    }

    #[test]
    fn fighting_and_taking() {
        assert_eq!(parse("attack Bryn"), Input::Attack("bryn".into()));
        assert_eq!(parse("hit cole"), Input::Attack("cole".into()));
        assert!(matches!(parse("attack"), Input::Unknown(_)));
        assert_eq!(parse("take"), Input::Do(Action::Take));
        assert_eq!(parse("loot"), Input::Do(Action::Take));
    }

    #[test]
    fn waiting_with_units() {
        assert_eq!(parse("wait"), Input::Wait(DEFAULT_WAIT));
        assert_eq!(parse("wait 30"), Input::Wait(Duration::seconds(30)));
        assert_eq!(parse("wait 45s"), Input::Wait(Duration::seconds(45)));
        assert_eq!(parse("z 10m"), Input::Wait(Duration::minutes(10)));
        assert_eq!(parse("wait 2h"), Input::Wait(Duration::hours(2)));
    }

    #[test]
    fn sleeping_waking_and_robbing() {
        assert_eq!(parse("sleep"), Input::Sleep(DEFAULT_SLEEP));
        assert_eq!(parse("rest 2h"), Input::Sleep(Duration::hours(2)));
        assert_eq!(parse("sleep 4d"), Input::Sleep(Duration::days(4)));
        assert_eq!(parse("sleep 999d"), Input::Sleep(MAX_SLEEP));
        assert!(matches!(parse("sleep soundly"), Input::Unknown(_)));
        assert_eq!(parse("wake"), Input::Do(Action::Wake));
        assert_eq!(parse("return"), Input::Do(Action::Return));
        assert_eq!(parse("relinquish"), Input::Do(Action::Relinquish));
        assert_eq!(parse("rob Ash"), Input::Rob("ash".into()));
        assert!(matches!(parse("rob"), Input::Unknown(_)));
    }

    #[test]
    fn waiting_is_capped_and_rejects_nonsense() {
        assert_eq!(parse("wait 99999h"), Input::Wait(MAX_WAIT));
        assert_eq!(parse("wait 18446744073709551615h"), Input::Wait(MAX_WAIT));
        for bad in ["wait 0", "wait soon", "wait -5", "wait m", "wait 1.5h"] {
            assert!(matches!(parse(bad), Input::Unknown(_)), "{bad}");
        }
    }

    #[test]
    fn views_and_housekeeping() {
        assert_eq!(parse("look"), Input::Look);
        assert_eq!(parse("x"), Input::Inspect);
        assert_eq!(parse("stats"), Input::Stats);
        assert_eq!(parse("i"), Input::Pack);
        assert_eq!(parse("?"), Input::Help);
        assert_eq!(parse("quit"), Input::Quit);
        assert_eq!(parse("   "), Input::Empty);
    }

    #[test]
    fn the_forgotten_seek_and_the_living_name_them() {
        assert_eq!(parse("seek"), Input::Do(Action::Seek));
        assert_eq!(parse("name Ash"), Input::Name("ash".into()));
        assert!(matches!(parse("name"), Input::Unknown(_)));
    }

    #[test]
    fn a_founders_names_keep_their_capitals() {
        assert_eq!(parse("CALL  The Salt March "), Input::Do(Action::NameLand("The Salt March".into())));
        assert_eq!(parse("house McAllister"), Input::Do(Action::NameHouse("McAllister".into())));
        assert_eq!(parse("invite Ada"), Input::Do(Action::Invite("Ada".into())));
        assert_eq!(parse("war The Salt March"), Input::Do(Action::Deal { with: "The Salt March".into(), dealing: Dealing::War }));
        assert_eq!(parse("ally bryn"), Input::Do(Action::Deal { with: "bryn".into(), dealing: Dealing::Ally }));
        assert!(matches!(parse("truce"), Input::Unknown(_)));
        assert!(matches!(parse("call"), Input::Unknown(_)));
        assert!(matches!(parse("invite   "), Input::Unknown(_)));
    }

    #[test]
    fn the_board_has_its_own_words() {
        let ore = Lot { resource: Resource::Ore, amount: 10 };
        let kelp = Lot { resource: Resource::Kelp, amount: 30 };
        assert_eq!(parse("post 10 ore for 30 kelp"), Input::Do(Action::Post { give: ore, want: kelp }));
        let glass = Lot { resource: Resource::Tideglass, amount: 2 };
        let wood = Lot { resource: Resource::Driftwood, amount: 50 };
        assert_eq!(parse("offer 2 glass for 50 wood"), Input::Do(Action::Post { give: glass, want: wood }));
        for nonsense in ["post", "post 10 ore", "post 10 ore to 30 kelp", "post ten ore for 30 kelp", "post 10 gold for 1 ore"] {
            assert!(matches!(parse(nonsense), Input::Unknown(_)), "{nonsense}");
        }
        assert_eq!(parse("board"), Input::Board);
        assert_eq!(parse("bounty Bryn 30 ore"), Input::Bounty("bryn".into(), Lot { resource: Resource::Ore, amount: 30 }));
        assert!(matches!(parse("bounty bryn"), Input::Unknown(_)));
        assert!(matches!(parse("bounty bryn 0 ore"), Input::Unknown(_)));
        assert_eq!(parse("accept 3"), Input::Do(Action::Accept(ContractId::new(3))));
        assert_eq!(parse("withdraw 0"), Input::Do(Action::Withdraw(ContractId::new(0))));
        assert!(matches!(parse("accept three"), Input::Unknown(_)));
        assert_eq!(parse("collect"), Input::Do(Action::Collect));
        assert_eq!(parse("tend Bryn"), Input::Tend("bryn".into()));
    }

    #[test]
    fn unknown_words_say_so() {
        let Input::Unknown(message) = parse("dance wildly") else { panic!("expected Unknown") };
        assert!(message.contains("dance"));
    }
}
