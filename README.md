# Equilibrium

A text-first MMO whose world is shaped by its players, built from small,
reusable Rust crates that compile natively for the server and to wasm for
the browser.

## Play

```sh
cargo run -p eq-cli
```

One character on the Rocky Shoreline: a camp and nine places to gather, two of them cut off at high tide. The rules support many characters; the terminal plays one. Walk, gather, wait and watch what your choices do to the place.

Working one deposit wears you out on it: each haul there gives you 15% less than a full one for every haul you took there lately, down to a tenth, though the deposit gives up as much as ever and the work teaches you as much. A haul's worth of that wears off for every minute you spend elsewhere, and it is yours alone: anyone else there gathers as if fresh. So whoever moves between deposits gathers at full value and pays for it in walking, and whoever stays strips the place for less and less.

Over-harvesting marks whoever does it. A marked character can be attacked in the wilderness (`attack <name>`); the loser dies and drops everything they carry beside the body, in a relic (`take`). A relic holding tide-glass is sealed to everyone but its owner until the body is gone or its guard is brought down. Something lives in the Deep Grotto and goes for whoever comes in (`attack lurker`). Nobody can be harmed at camp, and anybody can in the cave, the grotto and on the reef.

The dead walk as spirits the living cannot see. A spirit can only walk, `gather`, or `attack shade`. When you die that life is over. Your body stays where it fell with what you carried beside it, your spirit is carried off (to the River of Souls in a world with founders, to camp otherwise), and there is no going back into the body: nobody is even told where it lies. After ten minutes it rises as a guard over what you dropped. The one way on is the afterlife. Nothing can hurt a spirit, so it is the place to practise. `gather` where the living would work and you learn that work's skill from nothing, as slowly as anyone learning it for the first time. Where the living do not work, such as camp, there is a shade to fight instead: it never strikes back, another takes its place when it falls, and it teaches fighting. Go far enough in one skill and you are born again, already knowing it. How far grows with every death: level 10 before a first life (about twenty minutes), 15 after a first death (about thirty-five), then 20, 25 and so on up to 50 (over two hours). So each death takes longer to come back from, and sends you back knowing more. A body come back from the dead wakes weary, with no rested bonus until it has slept. Fighting is a skill for the living too: every blow teaches a little, and every two levels add a percent to a blow. Nobody skips it, and nobody chooses where they arise: it is the land with the fewest people, and that land is home from then on. Friends who want to find each other again have to be strong enough to cross what lies between.

Death itself keeps nothing of what you knew. But a life can be remembered: if nobody else takes what lay beside your body, your next life knows how high each skill stood, and practice counts four times over until it stands there again. New skills take as long as they ever did. Whoever stands over a body can loot what it dropped, which loses its owner that, or `bury <name>` instead. A burial takes a minute and puts the body and everything it dropped in a grave. No guard rises, and nothing can be taken while the grave is left alone, so a friend who buries you protects that life all the way through your afterlife. The grave stays for good, and anyone standing there can see whose it is. Anyone can `dig` it up, too. Whoever does has what was buried in it, and breaks the tie it kept: its owner stops learning fast what that life knew, whether they are still in the afterlife or living again. Dig up someone else's and the land marks you for a robber. Dig up your own, if you can remember where it is and get there, and you have your old belongings back at the price of your old life. Either way the grave's owner is changed by it, however much of that life they had already won back: five levels leave whichever of Vigor, Finesse and Resilience stands highest, and go to Attunement. Less of the body, more of the spirit. Each death soon after another holds the spirit back three minutes longer, and every death sickens the shoreline a little.

A spirit can `gather` too: it draws ectoplasm from any place the living could work. The living can tap the same leylines (`tap`), drawing as much as Attunement allows and learning Attunement for it, but the land pays: each tap takes 10 from the deposit, with the strain of taking it, and tapping one already under pressure taints you as greedy gathering does. Ectoplasm is the one thing you keep through death. At rebirth it is spent, twenty for each percent of the old life you keep outright (up to fifteen). Lineage grows with every life and raises it further, and dying where something can be worked keeps twice the share of that work's attribute. Draw a hundred ectoplasm over your lives and your spirit travels twice as fast.

Two people can mend what has been stripped. One begins a Restoration Rite over a hard-pressed deposit (`rite`), another answers within a minute and a half (`rite`): the strain eases, the deposit is brought back to half full, and for fifteen minutes it is warded, yielding nothing unless it is pristine and at least half full. The land will not answer anyone it has marked. This is what lets a careful majority hold a zone against people who strip it.

At camp there is a Contract Board for trading with people who are not there: `post 10 ore for 30 kelp` pins up an offer and the board holds what you offered; `board` lists what is up; `accept 3` pays a contract's price and takes what it holds; the price waits for whoever posted it to `collect`. `withdraw 3` takes your own down.

Being up wears off your rested bonus, and the rested learn faster. `sleep` puts it back, but sleep is only safe at camp: anywhere else a sleeper can be robbed (`rob <name>`), and robbing someone the land has not marked marks the robber. Sleep for more than three days and the body starts to waste, though what it loses comes back four times faster than it was first learned. Someone who looks after a sleeper (`tend <name>`) buys them a day for each tending, or two if the one tending is of their own land, up to two weeks more. Sleep for two weeks untended and the body dies where it lies: you wake a forgotten spirit with ten waking hours to `seek` three memories of yourself around the shoreline, or to be named at camp by someone living (`name <name>`). Run out of time and you fade, your name is cut in the camp's stones, and you begin again with almost nothing. There is a living to be made from other people's troubles. At camp, `bounty bryn 30 ore` puts a price on someone's head: the board holds it, `board` lists it, and it is paid to whoever kills them, to `collect` at that board. Until it is paid, whoever it is on can be struck anywhere but a haven, mark or no mark. A price must be at least 10 of something, and at least 100 on a newcomer (someone in their first life with no skill at 10 yet): possible, and not worth it. Being paid for a killing stains whoever takes the pay: the land's taint, two fifths of the way to its mark, fading over ten minutes, so three paid killings in a row make the hunter fair game. Whoever takes a price for a newcomer the land had no quarrel with is marked outright. Or hire out as a guard. At camp, `hire 40 ore to saltmarch` (or `to ground`, for the Meeting Ground) puts up a reward for whoever sees you safely there; `board` lists it. Anyone standing with you takes the job with `guard <your name>`, and is paid on the spot when you both stand where you were going. If you die on the way, or `dismiss` the job, the reward goes back to the board for you to collect; if your guard dies, the job is open again. It is how someone new gets through a pass. Type `help` in the game for commands.

- `--seed N` picks the world's random seed (default 1).
- `--bots N` puts N bots on the shoreline with you: mostly careful, with a greedy one among every four.
- `--verify` replays your command log when you quit and checks it rebuilds the same world.

`--generated` plays alone in a territory made from the seed, not the hand-built shoreline: a haven and nine places to work, laid out differently for every `--seed`, with the rare things furthest in and the tide across the way to them. Each holds exactly what the shoreline holds.

### With other people

```
cargo run -p eq-server -- --bots 12
cargo run -p eq-cli -- --connect ws://127.0.0.1:4000 --name Mikey --key "a passphrase"
```

The server holds one world and keeps it going whether anyone is connected or not: one tick a second, real time. Each player sees only what their character would notice. The first to use a name sets its passphrase; after that the name only answers to it.

Your character does not leave when you do. Close the connection and they stay on their feet for two minutes, then lie down to sleep where they stand, so where you stop for the night matters. `wait` does nothing here, and `sleep` lasts until you `wake`.

The world is saved in `realm/` as it goes (`--data DIR` to put it elsewhere): every command, in order, in numbered segments of the log (`commands.0.log`, `commands.1.log`, ...), and a snapshot of the whole world every world-hour. Stop the server with Ctrl-C and the next start loads the snapshot, replays only the commands after it, and carries on from the same tick. Delete `snapshot.json` and the whole log rebuilds the same world from the start. `--seed` and `--bots` only matter the first time. The server's bots are mostly careful, with wardens among them and one greedy roamer in every eight for them to deal with.

- `--founders N` (12 unless you say otherwise): the first N players to arrive uninvited each get a land of their own, made from the world's seed and joined to nothing. A founder names it (`call The Salt March`), names their house (`house Thorne`), can `raise` that house somewhere in the land out of 40 driftwood and 20 ore (under its roof the land's own people cannot be struck or robbed, awake or asleep, even in war; the ground it stands on is never worked again, so choose which of the land's nine places to give up; and keep it: if nobody of the land sets foot in it for a week it falls to ruin, shelters nobody, and half of what built it lies there for anyone to take, though the founder can raise another), and can `invite <name>`: whoever joins under that name arrives in the founder's land. The invited start alive, ready to play. Once every land is founded, everyone else arrives unborn: a spirit, who must practise to level 10 like any of the dead before being born wherever there are fewest people. It is a way to learn the game with nothing to lose. The unborn do not fade, however long they take. In a world with founders every soul enters the afterlife at the same place, the River of Souls: the unborn begin there, the forgotten wake there, and the dead are carried there once their bodies rise without them or are buried. Shades drift on its bank, and ways lead from it into every land (`first`, `second`, and so on) for a spirit that would sooner practise the work. Nothing leads back, and the living cannot come. Each founder's land has one way out: a pass at its far end, barred by a sentinel. Blows from anyone new glance off it, so no crowd of newcomers gets through. It takes Vigor and Resilience near 75 to break out alone, or two or three people part of the way there, and another sentinel rises half an hour later. Every pass leads `beyond` to the same place, the Meeting Ground: contested, the richest tide-glass in the world, and the only place where people of different lands meet. It is also where lands deal with each other, through their founders: `ally <land>` (it holds once the other founder offers the same), `war <land>` (takes only one), and `truce <land>` (leaves an alliance, or offers to end a war). Name a land by its own name or its founder's. Allies are let through by each other's sentinels. Lands at war may strike each other's people anywhere but a haven, and every death on either side sickens both lands. Lands that never deal with each other are nothing to each other, and nothing follows.

Pacts tip the whole world. Every standing alliance pushes it toward stillness and every standing war toward fury, a step each hour, and making one is felt at once. Too much peace and the world goes dry, then parched: what is taken from the ground comes back at half the pace, then not at all. Too much war and it grows restless, then raging: every twenty minutes a storm tears 3%, then 10%, from everything that can be worked, in every land. A parched world cannot keep its creatures either: lurkers die where they lie and their lairs stay empty until the drought breaks. In a restless or raging one they strike a quarter, then half, as hard again, and each storm takes 5, then 15, health from anyone caught outside a haven or their own house (never the last of it). A storm costs the land too, as a death does: a restless world holds every land a little under its best, and a raging one grinds them all down to nothing in a day or so, which is the Void Consumption. The tide follows the world too: in a dry world the tidal ways stay clear half again as long, in a parched one the water never comes in at all, in a restless one they are clear a quarter less, and in a raging one half as long, and the tide crashes in hard enough to take 10 health from anyone caught on the shore when it comes (never the last of it). As many wars as alliances, or none of either, and it eases back to even.

Everyone is born into one of six bloodlines, the one of the land they first arise in, and keeps it through every life wherever they are reborn. The lands take the bloodlines in turn as they are founded. Each leans toward two attributes, which its people learn a quarter faster (`stats` says which), and a founder can name theirs for good (`bloodline Ironkin`). A rite held by two people of different blood lifts all of a deposit's strain, where one bloodline alone lifts half. Each bloodline also has a gift and a wound, so being born to it is a reward and a cost at once (a heavy blow is one of 20 or more):

| Bloodline | Gift | Wound |
| --- | --- | --- |
| Tidal | Ebb and Flow: blows land 15% harder while the tide is in | Brine Corruption: a heavy blow leaves them bleeding for a minute instead of mending |
| Covenant | Structural Integrity: blows land 10% softer on them in any house | Petrifying Stasis: at 20 health or less every way takes them twice as long |
| Ash | Cinder Renewal: when they kill or are killed, allies standing by mend 8% of what they are missing | Ashen Lungs: the rested bonus burns away half again as fast |
| Stone | Earthen Mass: a quarter of their Resilience goes into every blow | Fissure Cracks: a heavy blow stops all mending for two minutes |
| Lunar | Lunar Sight: alive, they see spirits | Fragile Focus: a heavy blow scatters a fifth of their ectoplasm |
| Nomadic | Endless Horizon: every way out of the wild takes them a fifth less time | Restless Soul: after five minutes in a haven it mends them no faster than the wild |

Three pairs of bloodlines resonate when they stand together. Silt-Forming (Tidal and Stone): their blows land 15% harder there, but the mud holds them too, and leaving takes them half again as long. Glyph Ward (Covenant and Lunar in a house): nobody in that house can be struck or robbed, whatever their land, while the Lunar feed the ward one ectoplasm a minute. Wildfire Trail (Ash and Nomadic in the wild): both leave a third faster, and the fire burns 5% of the deposit they leave behind.

People of different lands can swear to one faction. A charter is opened in a house, which becomes its keep (`charter Saltward`), and others sign it there (`sign Saltward`). Once five have signed, of at least three bloodlines, it is a faction and holds its keep; after that anyone may sign to join. Members fill its vault at the keep (`give 20 ore`) and set markers where they stand (`claim`, 20 ore from the vault). Ground a faction holds gives its members something: they are told whenever someone from another land, not sworn to it, walks onto it, nobody can rob them asleep there, and they travel out of it twice as fast. At the keep itself they cannot be struck or robbed, whatever land they come from. And it costs them: every marker, the keep's included, takes 10 ore from the vault each day, and a vault that cannot pay ends the faction. Anyone else can `break` a marker: it takes ten minutes, the land marks them the moment they begin, every member is told, and the 20 ore that went into it is theirs if they finish. Breaking the keep's marker ends the faction. A member who leaves (`forsake`) is an oathbreaker for a week: nobody will take their oath, and those they left may strike them anywhere but a haven. If one of the five who signed it into being leaves, the faction ends then and there, and only the traitor pays. When a faction ends, half its vault spills at the keep for anyone to take and the rest is lost, and everyone who was sworn to it waits a week to swear again. `faction` shows how yours stands. `history` tells your land's story: its house raised or ruined, and its people born, killed and faded.

The world is also always in one of three eras, and everyone is told when it turns. The Era of Rebuilding, while the lands are well and nobody is at war: a house costs a quarter less, and the founder of a land takes a fifth of what every contract filled at its board earns (a tenth in other eras; nobody is taxed on their own board). The Era of Conflict, once any two lands are at war or the lands average under half their vitality: every blow teaches twice as much fighting, and anyone outside their own land can be struck anywhere but a haven, mark or no mark. The Void Consumption, when the lands average under a fifth: whatever is killed in the wild is back at once, anyone abroad is fair game, a haven shelters only its own land's people, and the Spirit Layer bleeds into the world: the living see the dead, the dead practise twice as fast, and every draw a spirit takes at a deposit takes 5 from the deposit too. In a world with founders, while the Void lasts it also raises a Behemoth in whichever founded land is worst off. It roams that land's open ground (never a haven or a house) and goes for anyone it finds. Its hide turns weak blows, and a blow lands for only a quarter for each bloodline that has struck it in the last ten minutes, so one land's people alone cannot scratch it and it takes four bloodlines to strike it in full (`attack behemoth`). Bring it down and the land it fell in is cleansed of what its dead cost it and its deposits mended, and a hundred tide-glass lie where it fell for anyone to take. Every life it ends heals a tenth of it and sickens every other land as a death would. Another rises an hour after one falls, for as long as the Void lasts. `--founders 0` puts everyone on the Rocky Shoreline with the bots.
- `--addr HOST:PORT` to listen somewhere other than `127.0.0.1:4000`.
- `--tick-ms N` to run world time faster or slower than real time.

The browser can join too. Add the server's address to the page's own: `…/index.html?server=ws://127.0.0.1:4000`. The page then runs no game of its own: it asks for a name and passphrase and draws what the server sends, with the same components. Serve the page from the machine the server is on (`cd web && npm run build && python3 -m http.server --directory dist`); a page loaded over https will generally refuse to talk to a plain `ws://` server elsewhere.

There is no encryption and no accounts beyond name and passphrase yet: run it on your own machine or network.

### In the browser

A basic top-down view: the same game compiled to WebAssembly, drawn by four [Zubra](https://github.com/zubra-js/zubra) components.

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
| `web/components/eq-actions.zb` | Buttons for exits, gathering, picking things up, attacking, robbing, sleeping and waiting. |
| `web/components/eq-log.zb` | What has happened. |
| `web/app.js` | Joins a game to a view. No DOM or wasm, so it is tested with `node --test`. |
| `web/main.js` | Loads the wasm and the components. |

CI builds the site and plays it in headless Chrome twice: alone (`web/test/e2e.html`) and as two players on a real server (`web/test/e2e-online.html`). In the first it presses buttons and map tiles, types a command, waits for the tide, then reloads with bots, watches them wander, hunts down the marked one, fights it and picks up what it drops, checking the page after each.

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
