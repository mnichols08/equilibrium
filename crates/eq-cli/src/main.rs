//! Play Equilibrium in a terminal.
//!
//! ```text
//! equilibrium [--seed N] [--verify]
//! ```
//!
//! `--verify` replays the session's command log when you quit and checks
//! that it rebuilds exactly the same world.
//!
//! The game itself is `eq_game::Solo`. This file only reads lines and
//! prints what comes back.

use std::io::{self, BufRead, IsTerminal, Write};
use std::process::ExitCode;

use eq_game::{narrate, Solo};

const USAGE: &str = "usage: equilibrium [--seed N] [--verify]";

const INTRO: &str = "\
EQUILIBRIUM
The shoreline gives what you take from it, and remembers how you took it.
Type \"help\" for commands.";

#[derive(Debug, PartialEq, Eq)]
struct Options {
    seed: u64,
    verify: bool,
}

fn parse_args(args: impl Iterator<Item = String>) -> Result<Options, String> {
    let mut options = Options { seed: 1, verify: false };
    let mut args = args.skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--verify" => options.verify = true,
            "--seed" => {
                let value = args.next().ok_or("--seed needs a number")?;
                options.seed = value.parse().map_err(|_| format!("\"{value}\" is not a valid seed"))?;
            }
            other => return Err(format!("unknown option \"{other}\"")),
        }
    }
    Ok(options)
}

/// The line that reports the replay check.
fn verdict(solo: &Solo, same: bool) -> String {
    let (commands, ticks) = (solo.commands(), solo.now().get());
    if same {
        format!("Replay check passed: {commands} commands over {ticks} ticks rebuilt the same world.")
    } else {
        format!("Replay check FAILED: {commands} commands over {ticks} ticks rebuilt a different world.")
    }
}

fn prompt(solo: &Solo, out: &mut impl Write) -> io::Result<()> {
    write!(out, "\n[{}] > ", narrate::clock(solo.now()))?;
    out.flush()
}

/// Plays until the input ends or the player quits. Returns `false` only
/// if `--verify` was asked for and the replay did not match.
fn run(options: &Options, input: impl BufRead, mut out: impl Write, echo: bool) -> io::Result<bool> {
    let mut solo = Solo::new(options.seed);
    writeln!(out, "{INTRO}\n\n{}", solo.look())?;
    prompt(&solo, &mut out)?;

    for line in input.lines() {
        let line = line?;
        if echo {
            writeln!(out, "{line}")?;
        }
        let reply = solo.handle(&line);
        for text in &reply.lines {
            writeln!(out, "{text}")?;
        }
        if reply.quit {
            break;
        }
        prompt(&solo, &mut out)?;
    }

    writeln!(out)?;
    if !options.verify {
        return Ok(true);
    }
    let same = solo.replays_identically();
    writeln!(out, "{}", verdict(&solo, same))?;
    Ok(same)
}

fn main() -> ExitCode {
    let options = match parse_args(std::env::args()) {
        Ok(options) => options,
        Err(problem) => {
            eprintln!("{problem}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    let stdin = io::stdin();
    let echo = !stdin.is_terminal();
    let mut out = io::stdout().lock();
    match run(&options, stdin.lock(), &mut out, echo) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("equilibrium: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn transcript(options: &Options, input: &str) -> (String, bool) {
        let mut out = Vec::new();
        let passed = run(options, input.as_bytes(), &mut out, true).unwrap();
        (String::from_utf8(out).unwrap(), passed)
    }

    #[test]
    fn a_session_reads_as_a_transcript() {
        let (text, passed) = transcript(&Options { seed: 1, verify: false }, "north\ngather\nquit\nlook\n");
        assert!(passed);
        assert!(text.starts_with("EQUILIBRIUM"));
        assert!(text.contains("Tidewatch Camp [haven]"));
        assert!(text.contains("north\nYou make your way to Ore Ledge."));
        assert!(text.contains("[0:00:30 low tide] > gather"));
        assert!(!text.contains("> look"), "nothing after quit should be read");
    }

    #[test]
    fn verify_reports_the_replay_check() {
        let (text, passed) = transcript(&Options { seed: 3, verify: true }, "east\ngather\nwait 5m\n");
        assert!(passed);
        assert!(text.contains("Replay check passed: 2 commands over 335 ticks rebuilt the same world."));
    }

    #[test]
    fn a_failed_replay_is_called_out() {
        assert!(verdict(&Solo::new(1), false).starts_with("Replay check FAILED"));
    }

    #[test]
    fn arguments_are_parsed_and_checked() {
        let args = |list: &[&str]| parse_args(list.iter().map(|arg| arg.to_string()));
        assert_eq!(args(&["eq", "--seed", "42", "--verify"]), Ok(Options { seed: 42, verify: true }));
        assert_eq!(args(&["eq"]), Ok(Options { seed: 1, verify: false }));
        assert!(args(&["eq", "--seed"]).is_err());
        assert!(args(&["eq", "--seed", "x"]).is_err());
        assert!(args(&["eq", "--fast"]).is_err());
    }
}
