//! Equilibrium for the browser.
//!
//! The same rules as the terminal game, compiled to WebAssembly. JavaScript
//! holds a [`Shore`], sends it the lines a player would type, and draws
//! the [`Scene`] it gives back. All the game logic stays in Rust.
//!
//! ```js
//! const shore = new Shore(1);
//! const lines = JSON.parse(shore.input("north"));   // what happened
//! const scene = JSON.parse(shore.scene());          // what to draw
//! ```

mod scene;

use eq_game::Solo;
use wasm_bindgen::prelude::*;

pub use scene::{Carried, DepositView, ExitView, Level, LinkView, PersonView, PlaceView, Scene, You};

/// One player's game, driven from JavaScript.
#[wasm_bindgen]
pub struct Shore {
    solo: Solo,
}

#[wasm_bindgen]
impl Shore {
    /// Starts a game. The same seed always plays out the same way.
    #[wasm_bindgen(constructor)]
    pub fn new(seed: u32) -> Self {
        Self { solo: Solo::new(u64::from(seed)) }
    }

    /// Handles one line of input, exactly as the terminal would, and
    /// returns what to show the player as a JSON array of strings.
    pub fn input(&mut self, line: &str) -> String {
        let lines = self.solo.handle(line).lines;
        serde_json::to_string(&lines).expect("lines of text are plain data")
    }

    /// The opening description, as plain text.
    pub fn look(&self) -> String {
        self.solo.look()
    }

    /// What to draw, as JSON. See [`Scene`] for the fields.
    pub fn scene(&self) -> String {
        Scene::of(self.solo.game(), Solo::YOU, self.solo.now()).map_or_else(|| "null".into(), |scene| scene.to_json())
    }

    /// Whether replaying every command so far rebuilds this exact world.
    pub fn replays_identically(&self) -> bool {
        self.solo.replays_identically()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_returns_what_happened_as_json_lines() {
        let mut shore = Shore::new(1);
        let lines: Vec<String> = serde_json::from_str(&shore.input("north")).unwrap();
        assert_eq!(lines[0], "You make your way to Ore Ledge.");
        let nothing: Vec<String> = serde_json::from_str(&shore.input("")).unwrap();
        assert!(nothing.is_empty());
    }

    #[test]
    fn the_scene_follows_the_input() {
        let mut shore = Shore::new(1);
        shore.input("east");
        let scene: serde_json::Value = serde_json::from_str(&shore.scene()).unwrap();
        assert_eq!(scene["you"]["place_name"], "Shingle Beach");
        assert_eq!(scene["clock"], "0:00:30");
        assert!(shore.look().starts_with("Shingle Beach"));
        assert!(shore.replays_identically());
    }
}
