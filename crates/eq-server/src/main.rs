//! Runs an Equilibrium server.
//!
//! ```text
//! equilibrium-server [--addr HOST:PORT] [--data DIR] [--seed N] [--bots N] [--founders N] [--tick-ms N]
//! ```
//!
//! The world is saved in `--data` as it goes and picked up again from
//! there on the next start. `--seed` and `--bots` only matter the first
//! time. One tick of world time passes every `--tick-ms` milliseconds
//! (1000 by default: world time is real time).

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use eq_server::net::Server;
use eq_server::{Config, Realm};

const USAGE: &str = "usage: equilibrium-server [--addr HOST:PORT] [--data DIR] [--seed N] [--bots N] [--founders N] [--tick-ms N]";

#[derive(Debug, PartialEq, Eq)]
struct Options {
    address: String,
    config: Config,
    tick_ms: u64,
}

/// The value that must follow `flag`.
fn value<T: std::str::FromStr>(flag: &str, given: Option<String>) -> Result<T, String> {
    let given = given.ok_or(format!("{flag} needs a value"))?;
    given.parse().map_err(|_| format!("\"{given}\" is not a valid value for {flag}"))
}

fn parse_args(args: impl Iterator<Item = String>) -> Result<Options, String> {
    let mut options = Options {
        address: "127.0.0.1:4000".into(),
        config: Config { seed: 1, bots: 0, founders: 6, data: Some(PathBuf::from("realm")) },
        tick_ms: 1_000,
    };
    let mut args = args.skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--addr" => options.address = value(&arg, args.next())?,
            "--data" => options.config.data = Some(value(&arg, args.next())?),
            "--seed" => options.config.seed = value(&arg, args.next())?,
            "--bots" => options.config.bots = value(&arg, args.next())?,
            "--founders" => options.config.founders = value(&arg, args.next())?,
            "--tick-ms" => options.tick_ms = value(&arg, args.next())?,
            other => return Err(format!("unknown option \"{other}\"")),
        }
    }
    if options.tick_ms == 0 {
        return Err("--tick-ms must be at least 1".into());
    }
    Ok(options)
}

#[tokio::main]
async fn main() -> ExitCode {
    let options = match parse_args(std::env::args()) {
        Ok(options) => options,
        Err(problem) => {
            eprintln!("{problem}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    let realm = match Realm::open(&options.config) {
        Ok(realm) => realm,
        Err(error) => {
            eprintln!("equilibrium-server: {error}");
            return ExitCode::FAILURE;
        }
    };
    println!("The world stands at tick {} with {} characters in it.", realm.now().get(), realm.game().population());
    let server = match Server::bind(realm, &options.address, Duration::from_millis(options.tick_ms)).await {
        Ok(server) => server,
        Err(error) => {
            eprintln!("equilibrium-server: could not listen on {}: {error}", options.address);
            return ExitCode::FAILURE;
        }
    };
    println!("Listening on ws://{}. Press Ctrl-C to save and stop.", server.address());
    server
        .run(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await;
    println!("Saved.");
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(line: &str) -> impl Iterator<Item = String> + '_ {
        line.split_whitespace().map(String::from)
    }

    #[test]
    fn defaults_are_a_local_saved_realm_in_real_time() {
        let options = parse_args(args("equilibrium-server")).unwrap();
        assert_eq!(options.address, "127.0.0.1:4000");
        assert_eq!(options.config, Config { seed: 1, bots: 0, founders: 6, data: Some(PathBuf::from("realm")) });
        assert_eq!(options.tick_ms, 1_000);
    }

    #[test]
    fn every_option_is_read() {
        let options = parse_args(args("s --addr 0.0.0.0:9 --data /tmp/w --seed 7 --bots 20 --founders 2 --tick-ms 50")).unwrap();
        assert_eq!(options.address, "0.0.0.0:9");
        assert_eq!(options.config, Config { seed: 7, bots: 20, founders: 2, data: Some(PathBuf::from("/tmp/w")) });
        assert_eq!(options.tick_ms, 50);
    }

    #[test]
    fn mistakes_are_named() {
        assert_eq!(parse_args(args("s --bots")).unwrap_err(), "--bots needs a value");
        assert_eq!(parse_args(args("s --seed x")).unwrap_err(), "\"x\" is not a valid value for --seed");
        assert_eq!(parse_args(args("s --fast")).unwrap_err(), "unknown option \"--fast\"");
        assert_eq!(parse_args(args("s --tick-ms 0")).unwrap_err(), "--tick-ms must be at least 1");
    }
}
