# Equilibrium

A text-first MMO whose world is shaped by its players, built from small,
reusable Rust crates that compile natively for the server and to wasm for
the browser.

## Play

```sh
cargo run -p eq-cli
```

One character on the Rocky Shoreline: a camp and nine places to gather, two of them cut off at high tide. The rules support many characters; the terminal plays one. Walk, gather, wait and watch what your choices do to the place.

Over-harvesting marks whoever does it. A marked character can be attacked in the wilderness (`attack <name>`); the loser goes down for five minutes and drops everything they carry (`take`). Nobody can be harmed at camp, and anybody can in the cave, the grotto and on the reef. Type `help` in the game for commands.

- `--seed N` picks the world's random seed (default 1).
- `--bots N` puts N bots on the shoreline with you: mostly careful, with a greedy one among every four.
- `--verify` replays your command log when you quit and checks it rebuilds the same world.

### In the browser

A basic top-down view: the same game compiled to WebAssembly, drawn by four [Zubra](https://github.com/journey-to-code-org/zubra) components.

Play it at <https://mnichols08.github.io/equilibrium/>. Every merge to `main` that passes CI is published there.

Three bots share the shoreline with you by default. Add `?bots=0` to the address to play alone, `?bots=8` for a crowd, or `?seed=7` for a different world.

To build it yourself:

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.130
cargo build -p eq-web --release --target wasm32-unknown-unknown
wasm-bindgen --target web --out-dir web/dist/pkg target/wasm32-unknown-unknown/release/eq_web.wasm
cd web && npm ci && npm run build
npx serve dist        # or any static file server
```

| Part | Job |
| --- | --- |
| `web/components/eq-map.zb` | The shoreline from above. Tiles are coloured by wear; press a neighbour to walk there. |
| `web/components/eq-status.zb` | Where you are, who else is there, the zone's vitality, your taint, pack and levels. |
| `web/components/eq-actions.zb` | Buttons for exits, gathering, picking things up, attacking and waiting. |
| `web/components/eq-log.zb` | What has happened. |
| `web/app.js` | Joins a game to a view. No DOM or wasm, so it is tested with `node --test`. |
| `web/main.js` | Loads the wasm and the components. |

CI builds the site and plays it in headless Chrome (`web/test/e2e.html`): it presses buttons and map tiles, types a command, waits for the tide, then reloads with bots, watches them wander, hunts down the marked one, fights it and picks up what it drops, checking the page after each.

The page holds no rules. It sends the game the same lines you would type in the terminal and draws the scene it gets back. `cargo run -p eq-web --example scene -- north gather` prints that scene.

## Crates

| Crate | Job |
| --- | --- |
| `eq-ids` | Typed ids that cannot be mixed up |
| `eq-rng` | Seeded, splittable, deterministic random source |
| `eq-clock` | Ticks, durations, cycles and pausable stopwatches |
| `eq-meter` | A bounded value with named bands |
| `eq-curve` | Deterministic, integer-only progression maths |
| `eq-ledger` | Point pools with a cap per entry and a cap overall |
| `eq-events` | Command log, tick-by-tick sessions and deterministic replay |
| `eq-world-graph` | Places joined by labelled exits that can open and close on a cycle |
| `eq-nodes` | Resource nodes that regrow, and wear out when over-harvested |
| `eq-skills` | Use-based proficiency with diminishing returns and a remembered peak |
| `eq-game` | The first playable build: rules for many characters, map, tuning, parser, narrator and bots |
| `eq-cli` | Terminal client for `eq-game` |
| `eq-web` | The game for the browser: a WebAssembly entry point and a drawable scene as JSON |

Every crate except `eq-cli` and `eq-web` is `no_std`, and all but `eq-cli` build for wasm. The crates above `eq-game` are usable without the game.

## Scale

Measured on one core with `cargo test --release -p eq-game --test scale -- --ignored --nocapture`:

- An idle world runs 10 to 12 million ticks per second: over 100 world-days per second.
- Replaying a crowd's command log runs at about 6 million commands per second, the same for 100 characters as for 5,000.
- `Game::audience(event)` names who should be told about an event from an index of who is where, so the cost of an event grows with the crowd around it and not with the whole world.

Not built yet, and needed before a real server:

- Saving: the command log and world state cannot be written to disk, and the log grows without limit. Needs snapshots.
- One zone only. `Zone` is its own type so that a world of many zones is the next step, not a rewrite.
- Characters cannot leave the world; ids are positions in a list.

## Ground rules

- Pure functions over plain data. No I/O, wall clock or global RNG in the core.
- Deterministic: the same seed and commands always give the same world.
- One tick is one second of world time.

## Develop

```sh
cargo test --workspace
cargo clippy --workspace --all-targets
rustup target add wasm32-unknown-unknown
cargo build --workspace --exclude eq-cli --target wasm32-unknown-unknown
```
