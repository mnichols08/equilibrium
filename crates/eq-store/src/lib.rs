//! An append-only command log on disk.
//!
//! A deterministic world can be rebuilt from its commands, so saving a
//! world means saving them: one JSON line per command, each with the
//! tick it was applied at, in order. Loading reads them back.
//!
//! The crate knows nothing about any game. A command is anything serde
//! can write and read.
//!
//! ```
//! use eq_store::LogFile;
//!
//! let path = std::env::temp_dir().join(format!("eq-store-doc-{}.log", std::process::id()));
//! # let _ = std::fs::remove_file(&path);
//! let (mut file, saved) = LogFile::<String>::open(&path)?;
//! assert!(saved.is_empty());
//! file.append(3, &"north".to_string())?;
//! file.append(9, &"gather".to_string())?;
//! drop(file);
//!
//! let (_, saved) = LogFile::<String>::open(&path)?;
//! assert_eq!(saved, [(3, "north".to_string()), (9, "gather".to_string())]);
//! # std::fs::remove_file(&path)?;
//! # Ok::<(), eq_store::StoreError>(())
//! ```

use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, BufReader, BufWriter, Write};
use std::marker::PhantomData;
use std::path::Path;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

/// Why a log could not be read or written.
#[derive(Debug)]
pub enum StoreError {
    /// The file could not be opened, read or written.
    Io(io::Error),
    /// A line was not a saved command. Holds the line's number, from 1.
    Corrupt {
        /// The line's number in the file, counting from 1.
        line: usize,
        /// What was wrong with it.
        problem: String,
    },
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "the log could not be read or written: {error}"),
            Self::Corrupt { line, problem } => write!(f, "line {line} of the log is not a saved command: {problem}"),
        }
    }
}

impl std::error::Error for StoreError {}

impl From<io::Error> for StoreError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// One saved line.
#[derive(Serialize, Deserialize)]
struct Line<C> {
    /// The tick the command was applied at.
    t: u64,
    /// The command.
    c: C,
}

/// A command log open for appending.
#[derive(Debug)]
pub struct LogFile<C> {
    out: BufWriter<File>,
    command: PhantomData<C>,
}

impl<C: Serialize + DeserializeOwned> LogFile<C> {
    /// Opens the log at `path`, creating it if it is not there, and
    /// returns it with every command already saved, oldest first.
    ///
    /// A last line cut short by a crash is dropped: it was never fully
    /// written, so it was never acknowledged. Damage anywhere else is an
    /// error, since quietly skipping a command would rebuild a different
    /// world.
    pub fn open(path: &Path) -> Result<(Self, Vec<(u64, C)>), StoreError> {
        let saved = if path.exists() { read(path)? } else { Vec::new() };
        // Rewrite what was read, so a cut-off tail does not stay in the file.
        let mut out = BufWriter::new(File::create(path)?);
        for (tick, command) in &saved {
            write_line(&mut out, *tick, command)?;
        }
        out.flush()?;
        drop(out);
        let file = OpenOptions::new().append(true).open(path)?;
        Ok((Self { out: BufWriter::new(file), command: PhantomData }, saved))
    }

    /// Saves one command, applied at `tick`, and flushes it to the file.
    pub fn append(&mut self, tick: u64, command: &C) -> Result<(), StoreError> {
        write_line(&mut self.out, tick, command)?;
        self.out.flush()?;
        Ok(())
    }
}

fn write_line<C: Serialize>(out: &mut impl Write, tick: u64, command: &C) -> Result<(), StoreError> {
    let line = serde_json::to_string(&Line { t: tick, c: command }).map_err(io::Error::from)?;
    writeln!(out, "{line}")?;
    Ok(())
}

fn read<C: DeserializeOwned>(path: &Path) -> Result<Vec<(u64, C)>, StoreError> {
    let lines: Vec<String> = BufReader::new(File::open(path)?).lines().collect::<Result<_, _>>()?;
    let mut saved = Vec::with_capacity(lines.len());
    for (index, text) in lines.iter().enumerate() {
        match serde_json::from_str::<Line<C>>(text) {
            Ok(line) => saved.push((line.t, line.c)),
            // Only the very last line may be cut short.
            Err(_) if index + 1 == lines.len() => break,
            Err(error) => return Err(StoreError::Corrupt { line: index + 1, problem: error.to_string() }),
        }
    }
    Ok(saved)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
    enum Move {
        Go(String),
        Gather,
    }

    /// A fresh path in the temp directory, removed when dropped.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!("eq-store-{}-{name}.log", std::process::id()));
            let _ = std::fs::remove_file(&path);
            Self(path)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    #[test]
    fn what_is_appended_comes_back_in_order() {
        let scratch = Scratch::new("order");
        let (mut file, saved) = LogFile::<Move>::open(&scratch.0).unwrap();
        assert!(saved.is_empty());
        file.append(1, &Move::Go("north".into())).unwrap();
        file.append(1, &Move::Gather).unwrap();
        file.append(40, &Move::Gather).unwrap();
        drop(file);

        let (mut file, saved) = LogFile::<Move>::open(&scratch.0).unwrap();
        assert_eq!(saved, [(1, Move::Go("north".into())), (1, Move::Gather), (40, Move::Gather)]);
        file.append(41, &Move::Gather).unwrap();
        drop(file);
        assert_eq!(LogFile::<Move>::open(&scratch.0).unwrap().1.len(), 4);
    }

    #[test]
    fn a_last_line_cut_short_is_dropped_and_does_not_come_back() {
        let scratch = Scratch::new("torn");
        let (mut file, _) = LogFile::<Move>::open(&scratch.0).unwrap();
        file.append(1, &Move::Gather).unwrap();
        drop(file);
        let mut raw = OpenOptions::new().append(true).open(&scratch.0).unwrap();
        write!(raw, "{{\"t\":2,\"c\":\"Gat").unwrap();
        drop(raw);

        let (mut file, saved) = LogFile::<Move>::open(&scratch.0).unwrap();
        assert_eq!(saved, [(1, Move::Gather)]);
        file.append(3, &Move::Gather).unwrap();
        drop(file);
        assert_eq!(LogFile::<Move>::open(&scratch.0).unwrap().1, [(1, Move::Gather), (3, Move::Gather)]);
    }

    #[test]
    fn damage_in_the_middle_is_an_error_not_a_skipped_command() {
        let scratch = Scratch::new("damaged");
        std::fs::write(&scratch.0, "{\"t\":1,\"c\":\"Gather\"}\nnonsense\n{\"t\":2,\"c\":\"Gather\"}\n").unwrap();
        let error = LogFile::<Move>::open(&scratch.0).unwrap_err();
        assert!(matches!(error, StoreError::Corrupt { line: 2, .. }), "{error}");
        assert!(error.to_string().contains("line 2"));
    }
}
