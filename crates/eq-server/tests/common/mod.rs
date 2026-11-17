//! Shared by the server's tests.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

use eq_game::CharacterId;
use eq_server::{Config, Delivery, Realm};

/// A fresh directory in the temp directory, removed when dropped.
pub struct Scratch(PathBuf);

impl Scratch {
    pub fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("eq-server-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        Self(path)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    /// A realm saved here.
    pub fn realm(&self, seed: u64, bots: usize) -> Realm {
        Realm::open(&Config { seed, bots, founders: 0, data: Some(self.0.clone()) }).unwrap()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A realm kept in memory only.
pub fn unsaved(seed: u64, bots: usize) -> Realm {
    Realm::open(&Config { seed, bots, founders: 0, data: None }).unwrap()
}

/// Everything `to` was told, as one string.
pub fn told(deliveries: &[Delivery], to: CharacterId) -> String {
    deliveries.iter().filter(|delivery| delivery.to == to).flat_map(|delivery| delivery.lines.clone()).collect::<Vec<_>>().join("\n")
}

/// Runs `ticks` ticks and returns everything anyone was told.
pub fn pass(realm: &mut Realm, ticks: u64) -> Vec<Delivery> {
    (0..ticks).flat_map(|_| realm.tick()).collect()
}
