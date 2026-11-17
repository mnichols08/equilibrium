//! Text to intent.
//!
//! The parser knows words, not the world: it does not check that an exit
//! exists or that there is anything to gather. The rules decide that.

use alloc::string::String;

use eq_clock::Duration;

use crate::game::Action;

/// How long `wait` lasts when no time is given.
pub const DEFAULT_WAIT: Duration = Duration::minutes(1);

/// The longest single `wait`.
pub const MAX_WAIT: Duration = Duration::days(1);

/// What a line of input asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Input {
    /// Have the player's character act.
    Do(Action),
    /// Let time pass.
    Wait(Duration),
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
        "wait" | "rest" | "z" => parse_wait(rest),
        "go" | "walk" | "enter" => match rest {
            Some(label) => go(label),
            None => Input::Unknown("Go where? Try \"go north\".".into()),
        },
        "north" | "south" | "east" | "west" | "up" | "down" | "cave" | "out" => go(verb),
        "n" | "s" | "e" | "w" | "u" | "d" => go(verb),
        _ => Input::Unknown(alloc::format!("I don't know how to \"{verb}\". Type \"help\" for commands.")),
    }
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

/// Accepts a bare number of seconds, or a number ending in `s`, `m` or `h`.
fn parse_wait(amount: Option<&str>) -> Input {
    let Some(amount) = amount else { return Input::Wait(DEFAULT_WAIT) };
    let (digits, unit_seconds) = match amount.as_bytes().last() {
        Some(b'h') => (&amount[..amount.len() - 1], 3_600),
        Some(b'm') => (&amount[..amount.len() - 1], 60),
        Some(b's') => (&amount[..amount.len() - 1], 1),
        _ => (amount, 1),
    };
    match digits.parse::<u64>() {
        Ok(count) if count > 0 => {
            let seconds = count.saturating_mul(unit_seconds).min(MAX_WAIT.as_ticks());
            Input::Wait(Duration::seconds(seconds))
        }
        _ => Input::Unknown("Wait how long? Try \"wait 30\", \"wait 10m\" or \"wait 2h\".".into()),
    }
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
    fn waiting_with_units() {
        assert_eq!(parse("wait"), Input::Wait(DEFAULT_WAIT));
        assert_eq!(parse("wait 30"), Input::Wait(Duration::seconds(30)));
        assert_eq!(parse("wait 45s"), Input::Wait(Duration::seconds(45)));
        assert_eq!(parse("rest 10m"), Input::Wait(Duration::minutes(10)));
        assert_eq!(parse("wait 2h"), Input::Wait(Duration::hours(2)));
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
    fn unknown_words_say_so() {
        let Input::Unknown(message) = parse("dance wildly") else { panic!("expected Unknown") };
        assert!(message.contains("dance"));
    }
}
