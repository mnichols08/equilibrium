//! One player's seat: a line of text, read from where their character
//! stands.
//!
//! The parser knows words. The seat knows the world well enough to turn
//! "attack bryn" into the character called Bryn, and to answer the
//! questions that cost no time. What to do with the result is up to
//! whoever holds the game: a single-player client lets time pass, a
//! server applies the action at the present tick.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use eq_clock::{Duration, Tick};

use crate::parser::{parse, Input};
use crate::{narrate, Action, Character, CharacterId, CreatureId, CreatureKind, Game};

/// What a line of input comes to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    /// Have the character do this.
    Act(Action),
    /// Let this much time pass.
    Wait(Duration),
    /// Lie down, sleep this long, and wake.
    Sleep(Duration),
    /// Nothing to do but say this.
    Say(Vec<String>),
    /// Stop playing.
    Quit,
}

/// Reads `line` as typed by the player of `who`.
pub fn interpret(game: &Game, who: CharacterId, now: Tick, line: &str) -> Request {
    let nobody_here = |name: &str| say(format!("There is nobody called \"{name}\" here."));
    match parse(line) {
        Input::Do(action) => Request::Act(action),
        // Only spirits can see shades, and a spirit can strike nothing else.
        Input::Attack(name) if name == "shade" && game.character(who).is_some_and(Character::is_spirit) => {
            Request::Act(Action::Spar)
        }
        Input::Attack(name) => match (someone_here_called(game, who, &name), creature_here_called(game, who, &name)) {
            (Some(target), _) => Request::Act(Action::Attack(target)),
            (None, Some(creature)) => Request::Act(Action::Fight(creature)),
            (None, None) => nobody_here(&name),
        },
        Input::Rob(name) => match someone_here_called(game, who, &name) {
            Some(target) => Request::Act(Action::Rob(target)),
            None => nobody_here(&name),
        },
        Input::Bury(name) => {
            let here = game.character(who).map(|you| you.place());
            let body = here.and_then(|here| game.bodies_at(here).find(|(_, body)| body.name().eq_ignore_ascii_case(&name)));
            match body {
                Some((target, _)) => Request::Act(Action::Bury(target)),
                None => say(format!("Nobody called \"{name}\" lies dead here.")),
            }
        }
        Input::Tend(name) => match someone_here_called(game, who, &name) {
            Some(target) => Request::Act(Action::Tend(target)),
            None => nobody_here(&name),
        },
        Input::Guard(name) => match someone_here_called(game, who, &name) {
            Some(client) => Request::Act(Action::Guard(client)),
            None => nobody_here(&name),
        },
        Input::Board => say(narrate::board(game, who)),
        Input::Faction => say(narrate::faction(game, who)),
        // A bounty can be on anyone in the world, here or not.
        Input::Bounty(name, reward) => match anyone_called(game, who, &name) {
            Some(on) => Request::Act(Action::Bounty { on, reward }),
            None if game.character(who).is_some_and(|me| me.name().eq_ignore_ascii_case(&name)) => {
                Request::Act(Action::Bounty { on: who, reward })
            }
            None => say(format!("There is nobody called \"{name}\" in the world.")),
        },
        Input::Name(name) => match anyone_called(game, who, &name) {
            Some(target) => Request::Act(Action::Name(target)),
            None => say(format!("You know of nobody called \"{name}\".")),
        },
        Input::Wait(time) => Request::Wait(time),
        Input::Sleep(time) => Request::Sleep(time),
        Input::Look => say(narrate::place(game, who, now)),
        Input::Inspect => say(narrate::inspect(game, who)),
        Input::Stats => say(narrate::stats(game, who)),
        Input::Pack => say(narrate::pack(game, who)),
        Input::Help => say(narrate::HELP.into()),
        Input::Unknown(reason) => say(reason),
        Input::Empty => Request::Say(Vec::new()),
        Input::Quit => Request::Quit,
    }
}

fn say(line: String) -> Request {
    Request::Say(alloc::vec![line])
}

/// The character standing with `who` whose name is `name`, whatever
/// its capitals.
fn someone_here_called(game: &Game, who: CharacterId, name: &str) -> Option<CharacterId> {
    let here = game.character(who)?.place();
    game.occupants(here).iter().copied().find(|&id| {
        id != who && game.character(id).is_some_and(|other| other.name().eq_ignore_ascii_case(name))
    })
}

/// The creature standing with `who` that answers to `word`: "lurker",
/// "guard" or "body".
fn creature_here_called(game: &Game, who: CharacterId, word: &str) -> Option<CreatureId> {
    let here = game.character(who)?.place();
    let answers = |kind: CreatureKind| kind.word() == word || (kind == CreatureKind::BodyGuard && word == "body");
    game.creatures_at(here).find(|creature| answers(creature.being.kind)).map(|creature| creature.id)
}

/// Anyone in the world but `who` whose name is `name`, seen or not.
fn anyone_called(game: &Game, who: CharacterId, name: &str) -> Option<CharacterId> {
    game.characters().find(|(id, other)| *id != who && other.name().eq_ignore_ascii_case(name)).map(|(id, _)| id)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ASH: CharacterId = CharacterId::new(0);
    const BRYN: CharacterId = CharacterId::new(1);

    fn read(game: &Game, who: CharacterId, line: &str) -> Request {
        interpret(game, who, Tick::ZERO, line)
    }

    #[test]
    fn names_are_read_from_where_the_speaker_stands() {
        let game = Game::new(1, &["Ash", "Bryn"]);
        assert_eq!(read(&game, ASH, "attack BRYN"), Request::Act(Action::Attack(BRYN)));
        assert!(matches!(read(&game, ASH, "attack shade"), Request::Say(_)), "the living see no shades");
        assert_eq!(read(&game, BRYN, "rob ash"), Request::Act(Action::Rob(ASH)));
        assert_eq!(read(&game, ASH, "attack ash"), say("There is nobody called \"ash\" here.".into()));
        assert_eq!(read(&game, ASH, "name bryn"), Request::Act(Action::Name(BRYN)));
        assert_eq!(read(&game, ASH, "name cole"), say("You know of nobody called \"cole\".".into()));
    }

    #[test]
    fn views_are_answered_at_once_for_the_one_who_asked() {
        let game = Game::new(1, &["Ash", "Bryn"]);
        let Request::Say(lines) = read(&game, BRYN, "look") else { panic!("expected words") };
        assert!(lines[0].contains("Also here: Ash."), "{lines:?}");
        assert_eq!(read(&game, ASH, "   "), Request::Say(Vec::new()));
        assert_eq!(read(&game, ASH, "quit"), Request::Quit);
    }

    #[test]
    fn time_and_plain_actions_pass_straight_through() {
        let game = Game::new(1, &["Ash"]);
        assert_eq!(read(&game, ASH, "wait 10m"), Request::Wait(Duration::minutes(10)));
        assert_eq!(read(&game, ASH, "sleep"), Request::Sleep(Duration::hours(8)));
        assert_eq!(read(&game, ASH, "gather"), Request::Act(Action::Gather));
        assert_eq!(read(&game, ASH, "attack lurker"), say("There is nobody called \"lurker\" here.".into()));
    }
}
