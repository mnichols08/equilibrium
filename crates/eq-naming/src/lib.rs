//! Names that players give to things.
//!
//! A world that lets its players name its lands and houses needs those
//! names checked in one place: not too short or long, made of letters,
//! not already taken, and not on a list of words the world will not
//! carry. This crate does that and nothing else. It knows nothing about
//! any game.
//!
//! The blocklist is the moderation hook: the caller supplies it. A name
//! is refused if any blocked word appears anywhere in it, whatever its
//! capitals or spacing.
//!
//! ```
//! use eq_naming::{check, Problem, Rules};
//!
//! let rules = Rules { shortest: 3, longest: 24, words: true };
//! let taken = ["Thornwatch"];
//! assert_eq!(check("  the   salt MARCH ", &rules, taken, &[]), Ok("The Salt March".to_string()));
//! assert_eq!(check("thornwatch", &rules, taken, &[]), Err(Problem::Taken));
//! assert_eq!(check("Bad Word Bay", &rules, taken, &["badword"]), Err(Problem::Blocked));
//! ```

#![no_std]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

/// What a name may be.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rules {
    /// The fewest characters, counting the spaces between words.
    pub shortest: usize,
    /// The most characters, counting the spaces between words.
    pub longest: usize,
    /// Whether a name may be several words. If not, a space is a
    /// character it may not hold.
    pub words: bool,
}

/// Why a name was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Problem {
    /// Shorter than the rules allow.
    TooShort,
    /// Longer than the rules allow.
    TooLong,
    /// Holds something other than letters, apostrophes and hyphens
    /// (and spaces, where several words are allowed).
    BadCharacter(char),
    /// Holds a word from the blocklist.
    Blocked,
    /// Already in use, whatever its capitals.
    Taken,
}

/// Checks `raw` against `rules`, the names already `taken` and the
/// `blocked` words, and returns it tidied: trimmed, single spaces
/// between words, each word starting with a capital.
pub fn check<'a>(
    raw: &str,
    rules: &Rules,
    taken: impl IntoIterator<Item = &'a str>,
    blocked: &[&str],
) -> Result<String, Problem> {
    let words: Vec<&str> = raw.split_whitespace().collect();
    let joined = words.join(" ");
    if let Some(bad) = joined.chars().find(|&letter| !allowed(letter, rules.words)) {
        return Err(Problem::BadCharacter(bad));
    }
    let length = joined.chars().count();
    if length < rules.shortest {
        return Err(Problem::TooShort);
    }
    if length > rules.longest {
        return Err(Problem::TooLong);
    }
    let bare = squashed(&joined);
    if blocked.iter().any(|word| !word.is_empty() && bare.contains(&squashed(word))) {
        return Err(Problem::Blocked);
    }
    if taken.into_iter().any(|other| squashed(other) == bare) {
        return Err(Problem::Taken);
    }
    Ok(words.iter().map(|word| capitalised(word)).collect::<Vec<_>>().join(" "))
}

fn allowed(letter: char, words: bool) -> bool {
    letter.is_alphabetic() || letter == '\'' || letter == '-' || (words && letter == ' ')
}

/// The name with only its letters, lower-cased: what two names must
/// differ in to count as different.
fn squashed(name: &str) -> String {
    name.chars().filter(|letter| letter.is_alphabetic()).flat_map(char::to_lowercase).collect()
}

/// The word with its first letter a capital and the rest as given,
/// unless it was all capitals or all lower case, which reads as shouting
/// or as not having tried: then the rest is lower-cased.
fn capitalised(word: &str) -> String {
    let mut letters = word.chars();
    let Some(first) = letters.next() else { return String::new() };
    let rest: String = letters.collect();
    let uniform = rest.chars().all(|letter| !letter.is_uppercase()) || rest.chars().all(|letter| !letter.is_lowercase());
    let rest = if uniform { rest.to_lowercase() } else { rest };
    first.to_uppercase().chain(rest.chars()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;

    const LAND: Rules = Rules { shortest: 3, longest: 24, words: true };
    const PERSON: Rules = Rules { shortest: 3, longest: 16, words: false };

    fn land(raw: &str) -> Result<String, Problem> {
        check(raw, &LAND, [], &[])
    }

    #[test]
    fn names_are_tidied() {
        assert_eq!(land("  the   salt MARCH "), Ok("The Salt March".to_string()));
        assert_eq!(land("gull's rest"), Ok("Gull's Rest".to_string()));
        assert_eq!(land("stone-under-sky"), Ok("Stone-under-sky".to_string()));
        assert_eq!(land("McAllister reach"), Ok("McAllister Reach".to_string()), "capitals inside a word are kept");
        assert_eq!(land("Étoile"), Ok("Étoile".to_string()));
    }

    #[test]
    fn length_counts_the_tidied_name() {
        assert_eq!(land("ab"), Err(Problem::TooShort));
        assert_eq!(land("   "), Err(Problem::TooShort));
        assert_eq!(land("a     b"), Ok("A B".to_string()), "three characters once tidied");
        assert_eq!(land(&"a".repeat(24)).map(|name| name.len()), Ok(24));
        assert_eq!(land(&"a".repeat(25)), Err(Problem::TooLong));
    }

    #[test]
    fn only_letters_apostrophes_hyphens_and_spaces_between_words() {
        assert_eq!(land("fort 9"), Err(Problem::BadCharacter('9')));
        assert_eq!(land("<b>bold</b>"), Err(Problem::BadCharacter('<')));
        assert_eq!(land("two\nlines"), Ok("Two Lines".to_string()), "any white space is just a gap between words");
        assert_eq!(check("two words", &PERSON, [], &[]), Err(Problem::BadCharacter(' ')));
        assert_eq!(check("ada", &PERSON, [], &[]), Ok("Ada".to_string()));
    }

    #[test]
    fn a_taken_name_is_taken_whatever_its_capitals_spacing_or_marks() {
        let taken = ["The Salt March", "Thornwatch"];
        assert_eq!(check("the salt march", &LAND, taken, &[]), Err(Problem::Taken));
        assert_eq!(check("Thorn Watch", &LAND, taken, &[]), Err(Problem::Taken));
        assert_eq!(check("thorn-watch", &LAND, taken, &[]), Err(Problem::Taken));
        assert_eq!(check("Thornwatcher", &LAND, taken, &[]), Ok("Thornwatcher".to_string()));
    }

    #[test]
    fn blocked_words_are_caught_inside_names_and_across_spaces() {
        let blocked = ["grot", ""];
        assert_eq!(check("Grotto Bay", &LAND, [], &blocked), Err(Problem::Blocked));
        assert_eq!(check("big ro tten", &LAND, [], &["grot"]), Err(Problem::Blocked), "spacing does not hide it");
        assert_eq!(check("Green Bay", &LAND, [], &blocked), Ok("Green Bay".to_string()), "an empty entry blocks nothing");
    }
}
