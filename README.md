# Equilibrium

A text-first MMO whose world is shaped by its players, built from small,
reusable Rust crates that compile natively for the server and to wasm for
the browser.

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

Every crate is `no_std` and usable without the game. `eq-events` and `eq-world-graph` need `alloc`.

## Ground rules

- Pure functions over plain data. No I/O, wall clock or global RNG in the core.
- Deterministic: the same seed and commands always give the same world.
- One tick is one second of world time.

## Develop

```sh
cargo test --workspace
cargo clippy --workspace --all-targets
rustup target add wasm32-unknown-unknown
cargo build --workspace --target wasm32-unknown-unknown
```
