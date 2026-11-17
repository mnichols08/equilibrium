# Equilibrium

A text-first MMO whose world is shaped by its players, built from small,
reusable Rust crates that compile natively for the server and to wasm for
the browser.

## Play

```sh
cargo run -p eq-cli
```

One character on the Rocky Shoreline (the rules support many; the terminal plays one). Walk, gather, wait and watch what your choices do to the place. Type `help` in the game for commands.

- `--seed N` picks the world's random seed (default 1).
- `--verify` replays your command log when you quit and checks it rebuilds the same world.

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
| `eq-game` | The first playable build: rules for many characters, map, tuning, parser and narrator |
| `eq-cli` | Terminal client for `eq-game` |

Every crate except `eq-cli` is `no_std` and builds for wasm. The crates above `eq-game` are usable without the game.

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
