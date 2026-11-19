//! The world as one player sees it from above, as plain data.
//!
//! A [`Scene`] is everything a client needs to draw one frame: where the
//! places are, how they join, who is where, and how the player stands. It
//! holds no rules and no references into the game, so it can be turned
//! into JSON and handed to anything that can draw.
//!
//! For now a scene shows the whole zone. What a character can actually
//! perceive from where they stand is a later rule.

use eq_clock::Tick;
use eq_game::content::{LOW_TIDE, MEMORIES_TO_REDEEM, REDEMPTION_CLOCK, TIDE};
use eq_game::{
    narrate, pressure_percent, Attribute, CharacterId, Creature, Death, Deposit, Game, Resource, SkillKind, Standing,
};
use eq_nodes::Quality;
use eq_world_graph::{Passage, PlaceId};
use serde::Serialize;

/// One frame of the game for one viewer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Scene {
    /// World time as `h:mm:ss`.
    pub clock: String,
    /// `"low"` or `"high"`.
    pub tide: &'static str,
    /// The era the world is in, by name.
    pub era: &'static str,
    /// What the world at large is like: `"parched"`, `"dry"`, `"even"`,
    /// `"restless"` or `"raging"`.
    pub mood: &'static str,
    /// Zone vitality, 0 to 100.
    pub vitality: i64,
    /// The zone's vitality band, as shown to players.
    pub health: &'static str,
    /// What its founder called the land the viewer is in, if it has a name.
    pub land: Option<String>,
    /// What its founder called their house, if it has a name.
    pub house: Option<String>,
    /// The viewer.
    pub you: You,
    /// Everyone else standing where the viewer is.
    pub company: Vec<PersonView>,
    /// Every place in the zone.
    pub places: Vec<PlaceView>,
    /// Every way between two places, once each.
    pub links: Vec<LinkView>,
    /// The ways out of where the viewer stands.
    pub exits: Vec<ExitView>,
}

/// The viewer's own character.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct You {
    /// The id of the place they are in.
    pub place: u64,
    /// That place's name.
    pub place_name: &'static str,
    /// What they see there.
    pub description: &'static str,
    /// Whether there is enough here to gather.
    pub can_gather: bool,
    /// Whether something lies on the ground here to pick up.
    pub can_take: bool,
    /// Whether they are lying down, asleep or settling.
    pub asleep: bool,
    /// Whether they could take part in a rite here: the deposit is hard
    /// pressed, unwarded, and the land has not marked them.
    pub can_rite: bool,
    /// Whether someone else has begun a rite here that waits for an answer.
    pub rite_waiting: bool,
    /// Health, 0 to 100.
    pub health: u32,
    /// Which life this is, counting the first as 1.
    pub life: u32,
    /// What their bloodline is called.
    pub bloodline: String,
    /// Their body, if they have died and are walking as a spirit.
    pub body: Option<BodyView>,
    /// Their redemption, if they slept too long and are a forgotten
    /// spirit, or have not been born yet.
    pub forgotten: Option<ForgottenView>,
    /// Whether there is a shade here for them to fight: they are a
    /// waking spirit, and the living do not work this place.
    pub can_spar: bool,
    /// How far a spirit has come in the afterlife. Any spirit has one.
    pub afterlife: Option<AfterlifeView>,
    /// The names of the dead whose bodies lie here and could be buried.
    pub can_bury: Vec<String>,
    /// Ectoplasm held: the one thing death does not take.
    pub ectoplasm: u64,
    /// Their spirit tier, by name.
    pub spirit_tier: &'static str,
    /// How rested they are, 0 to 100. The rested learn faster.
    pub rested: u32,
    /// Taint, 0 to 100.
    pub taint: u32,
    /// Whether the land has marked them.
    pub marked: bool,
    /// Skills with their levels.
    pub skills: Vec<Level>,
    /// Attributes with their levels.
    pub attributes: Vec<Level>,
    /// What they carry, leaving out what they have none of.
    pub pack: Vec<Carried>,
}

/// A named level.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Level {
    /// The skill or attribute.
    pub name: &'static str,
    /// Its level.
    pub level: u32,
}

/// An amount of one resource.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Carried {
    /// The resource.
    pub name: &'static str,
    /// How much.
    pub amount: u64,
}

/// One place on the map.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PlaceView {
    /// Its id.
    pub id: u64,
    /// Its name.
    pub name: &'static str,
    /// Column on the map; grows east.
    pub x: i32,
    /// Row on the map; grows south.
    pub y: i32,
    /// `"haven"`, `"wilderness"` or `"contested"`.
    pub safety: &'static str,
    /// Whether this land's house stands here.
    pub hall: bool,
    /// The names of those buried here.
    pub graves: Vec<String>,
    /// The faction whose marker stands here, if any.
    pub held_by: Option<String>,
    /// Whether the viewer is here.
    pub here: bool,
    /// What can be gathered here, if anything.
    pub deposit: Option<DepositView>,
    /// Who is here.
    pub people: Vec<PersonView>,
    /// Whose bodies lie here.
    pub bodies: Vec<String>,
    /// Whether a relic lies here.
    pub spoils: bool,
    /// The creatures here.
    pub creatures: Vec<FoeView>,
}

/// The state of a deposit.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DepositView {
    /// What it gives.
    pub resource: &'static str,
    /// How full it is, 0 to 100.
    pub stock_percent: u32,
    /// `"pristine"`, `"healthy"`, `"worn"` or `"stripped"`.
    pub quality: &'static str,
    /// How hard it is being pushed, 0 to 100.
    pub pressure: u32,
}

/// What has become of the body the viewer left. It does not say where:
/// there is no going back to it, and nobody is told where they fell.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct BodyView {
    /// `"lying"` where it fell, `"risen"` as a guard, or `"buried"`.
    pub state: &'static str,
}

/// A creature standing in a place.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct FoeView {
    /// What to call it: "lurker", or "Bryn's body".
    pub name: String,
    /// The word that names it in a command: `attack lurker`.
    pub word: &'static str,
    /// Its health.
    pub health: u32,
    /// Whether it goes for anyone it finds.
    pub aggressive: bool,
}

/// Any spirit's way back to the living: practise a skill where the
/// living would work, far enough, and live again knowing it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AfterlifeView {
    /// The best level reached in any one skill since leaving the living.
    pub level: u32,
    /// The level at which the spirit lives again.
    pub needed: u32,
}

/// A forgotten spirit's way back.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ForgottenView {
    /// Whether the spirit has never lived. The unborn do not fade.
    pub unborn: bool,
    /// Whether the spirit lies dormant and must wake before anything else.
    pub dormant: bool,
    /// Memories recovered so far.
    pub memories: u32,
    /// Memories needed to be redeemed.
    pub needed: u32,
    /// Waking seconds left before the spirit fades.
    pub time_left: u64,
    /// Whether there is a memory to seek where the spirit stands.
    pub can_seek: bool,
}

/// Someone standing in a place.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PersonView {
    /// Their name.
    pub name: String,
    /// Whether this is the viewer.
    pub you: bool,
    /// Whether the land has marked them.
    pub marked: bool,
    /// Whether they are asleep.
    pub asleep: bool,
    /// Whether the viewer may attack them right now.
    pub can_attack: bool,
    /// Whether the viewer may go through their pack right now.
    pub can_rob: bool,
}

/// A way between two places, drawn once whichever way it is walked.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct LinkView {
    /// One end.
    pub from: u64,
    /// The other end.
    pub to: u64,
    /// Whether it opens and closes with the tide.
    pub tidal: bool,
    /// Whether it can be walked right now.
    pub open: bool,
}

/// A way out of where the viewer stands.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ExitView {
    /// What to type or press to take it.
    pub label: String,
    /// The id of the place it leads to.
    pub to: u64,
    /// That place's name.
    pub to_name: &'static str,
    /// Whether it can be walked right now.
    pub open: bool,
}

impl Scene {
    /// Builds the scene for `viewer` at `now`, or `None` if they are not
    /// in the world.
    pub fn of(game: &Game, viewer: CharacterId, now: Tick) -> Option<Self> {
        let character = game.character(viewer)?;
        let here = character.place();
        let place = game.place_of(viewer)?;
        let you = You {
            place: here.get(),
            place_name: place.name,
            description: place.description,
            // The living need stock to work; a waking spirit only needs the place.
            can_gather: match (character.is_spirit(), place.deposit) {
                (false, Some(deposit)) => is_workable(&deposit),
                (true, Some(_)) => !character.is_dormant(),
                (_, None) => false,
            },
            can_take: game.can_take(viewer),
            asleep: character.is_lying_down(),
            can_rite: !character.is_spirit()
                && character.standing() != Standing::Marked
                && place.deposit.is_some_and(|deposit| deposit.needs_rite(now)),
            rite_waiting: place.deposit.and_then(|deposit| deposit.rite_open(now)).is_some_and(|beginner| beginner != viewer),
            health: character.health(),
            life: character.life(),
            bloodline: game.bloodline_name(character.bloodline()).to_owned(),
            body: character.death().map(body_view),
            can_spar: character.is_spirit() && !character.is_dormant() && place.deposit.is_none(),
            afterlife: character
                .is_spirit()
                .then(|| AfterlifeView { level: character.best_practised(), needed: character.afterlife_goal() }),
            can_bury: if character.is_spirit() {
                Vec::new()
            } else {
                game.bodies_at(here).map(|(_, body)| body.name().to_owned()).collect()
            },
            forgotten: character.forgotten().map(|forgotten| ForgottenView {
                unborn: forgotten.unborn,
                dormant: forgotten.dormant,
                memories: forgotten.memories(),
                needed: MEMORIES_TO_REDEEM,
                time_left: REDEMPTION_CLOCK.saturating_sub(forgotten.played).as_ticks(),
                can_seek: !forgotten.unborn
                    && !forgotten.dormant
                    && place.deposit.is_some()
                    && !forgotten.found_in.contains(&here),
            }),
            ectoplasm: character.ectoplasm(),
            spirit_tier: character.spirit_tier().name(),
            rested: character.rested(),
            taint: character.taint(),
            marked: character.standing() == Standing::Marked,
            skills: SkillKind::ALL
                .map(|skill| Level { name: skill.name(), level: character.skill(skill).level() })
                .into(),
            attributes: Attribute::ALL
                .map(|attribute| Level { name: attribute.name(), level: character.attribute(attribute).level() })
                .into(),
            pack: Resource::ALL
                .into_iter()
                .map(|resource| Carried { name: resource.name(), amount: character.carried(resource) })
                .filter(|carried| carried.amount > 0)
                .collect(),
        };
        Some(Self {
            clock: clock(now),
            tide: if TIDE.is_within(now, LOW_TIDE) { "low" } else { "high" },
            era: game.era().name(),
            mood: game.balance().mood().name(),
            vitality: game.zone_at(here).vitality(),
            health: game.zone_at(here).health().name(),
            land: game.zone_at(here).name().map(String::from),
            house: game.zone_at(here).house().map(String::from),
            you,
            company: people(game, viewer, here, now).into_iter().filter(|person| !person.you).collect(),
            places: places(game, viewer, here, now),
            links: links(game, here, now),
            exits: exits(game, here, now),
        })
    }

    /// The scene as JSON.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("a scene holds only plain data")
    }
}

fn body_view(death: &Death) -> BodyView {
    let state = match (death.buried, death.risen) {
        (true, _) => "buried",
        (false, true) => "risen",
        (false, false) => "lying",
    };
    BodyView { state }
}


fn foe_view(game: &Game, creature: &Creature) -> FoeView {
    let kind = creature.being.kind;
    let owner = creature.being.of.and_then(|owner| game.character(owner));
    FoeView {
        name: owner.map_or_else(|| kind.word().to_owned(), |owner| format!("{}'s body", owner.name())),
        word: kind.word(),
        health: creature.health(),
        aggressive: kind.is_aggressive(),
    }
}

fn is_workable(deposit: &Deposit) -> bool {
    deposit.node.stock() >= eq_game::content::MIN_WORKABLE_STOCK
}

/// The time part of the narrator's clock, without the tide.
fn clock(now: Tick) -> String {
    let full = narrate::clock(now);
    full.split(' ').next().unwrap_or_default().to_owned()
}

/// The places of the land the viewer is in. Other lands are not on
/// their map: nothing joins them.
fn places(game: &Game, viewer: CharacterId, here: PlaceId, now: Tick) -> Vec<PlaceView> {
    let land = game.zone_at(here);
    game.map()
        .places()
        .filter(|(id, _)| land.contains(*id))
        .map(|(id, place)| PlaceView {
            id: id.get(),
            name: place.name,
            x: place.position.0,
            y: place.position.1,
            safety: place.safety.name(),
            here: id == here,
            deposit: place.deposit.as_ref().map(deposit_view),
            people: people(game, viewer, id, now),
            hall: game.zone_at(id).hall() == Some(id),
            graves: game.graves_at(id).filter_map(|grave| game.character(grave.owner)).map(|owner| owner.name().to_owned()).collect(),
            held_by: game.factions().holding(id).map(|faction| faction.name.clone()),
            bodies: game.bodies_at(id).map(|(_, body)| body.name().to_owned()).collect(),
            spoils: game.relics_at(id).next().is_some(),
            creatures: game.creatures_at(id).map(|creature| foe_view(game, creature)).collect(),
        })
        .collect()
}

fn deposit_view(deposit: &Deposit) -> DepositView {
    let node = &deposit.node;
    DepositView {
        resource: deposit.resource.name(),
        stock_percent: if node.capacity() == 0 { 0 } else { node.stock() * 100 / node.capacity() },
        quality: quality_name(node.quality()),
        pressure: pressure_percent(node),
    }
}

const fn quality_name(quality: Quality) -> &'static str {
    match quality {
        Quality::Pristine => "pristine",
        Quality::Healthy => "healthy",
        Quality::Worn => "worn",
        Quality::Depleted => "stripped",
    }
}

fn people(game: &Game, viewer: CharacterId, place: PlaceId, now: Tick) -> Vec<PersonView> {
    game.occupants(place)
        .iter()
        .filter(|&&id| game.can_see(viewer, id))
        .filter_map(|&id| Some((id, game.character(id)?)))
        .map(|(id, character)| PersonView {
            name: character.name().to_owned(),
            you: id == viewer,
            marked: character.standing() == Standing::Marked,
            asleep: character.is_asleep(now),
            can_attack: game.can_attack(viewer, id).is_ok(),
            can_rob: game.can_rob(viewer, id, now).is_ok(),
        })
        .collect()
}

/// Each two-way path once, taken from its lower-numbered end.
fn links(game: &Game, here: PlaceId, now: Tick) -> Vec<LinkView> {
    let land = game.zone_at(here);
    game.map()
        .all_exits()
        // A way out of the land is an exit, but it is not on the land's map.
        .filter(|(from, exit)| *from < exit.to && land.contains(*from) && land.contains(exit.to))
        .map(|(from, exit)| LinkView {
            from: from.get(),
            to: exit.to.get(),
            tidal: !matches!(exit.passage, Passage::Open),
            open: exit.passage.is_open(now),
        })
        .collect()
}

fn exits(game: &Game, here: PlaceId, now: Tick) -> Vec<ExitView> {
    game.map()
        .exits(here)
        .iter()
        .map(|exit| ExitView {
            label: exit.label.clone(),
            to: exit.to.get(),
            to_name: game.map().place(exit.to).map_or("", |place| place.name),
            open: exit.passage.is_open(now),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use eq_game::Solo;

    fn scene(solo: &Solo) -> Scene {
        Scene::of(solo.game(), Solo::YOU, solo.now()).unwrap()
    }

    #[test]
    fn a_fresh_scene_shows_the_whole_shoreline_with_you_at_camp() {
        let scene = scene(&Solo::new(1));
        assert_eq!((scene.clock.as_str(), scene.tide), ("0:00:00", "low"));
        assert_eq!((scene.vitality, scene.health), (100, "peak health"));
        assert_eq!(scene.places.len(), 10);
        assert_eq!(scene.links.len(), 9);
        assert_eq!(scene.you.place_name, "Tidewatch Camp");
        assert!(!scene.you.can_gather);
        assert!(scene.you.pack.is_empty());
        assert!(scene.company.is_empty());

        let here: Vec<&PlaceView> = scene.places.iter().filter(|place| place.here).collect();
        assert_eq!(here.len(), 1);
        let you = PersonView {
            name: "You".into(),
            you: true,
            marked: false,
            asleep: false,
            can_attack: false,
            can_rob: false,
        };
        assert_eq!(scene.you.rested, 100);
        assert_eq!(here[0].people, [you]);
        assert_eq!((scene.you.health, scene.you.life, scene.you.can_take), (100, 1, false));
        assert_eq!((scene.you.body.clone(), scene.you.forgotten.clone()), (None, None));
        assert_eq!((scene.you.ectoplasm, scene.you.spirit_tier), (0, "Lost Shade"));
        assert!(scene.places.iter().all(|place| place.bodies.is_empty()));
        assert!(scene.places.iter().all(|place| !place.spoils));
    }

    #[test]
    fn the_dead_see_their_body_and_the_living_see_only_the_body() {
        use eq_events::World;
        use eq_clock::Duration;
        use eq_game::content::{ATTACK_TIME, GATHER_TIME};
        use eq_game::Command;

        let (ash, bryn) = (CharacterId::new(0), CharacterId::new(1));
        let mut game = Game::new(1, &["Ash", "Bryn"]);
        let mut events = Vec::new();
        game.apply(&Command::go(ash, "north"), Tick::new(0), &mut events);
        game.apply(&Command::go(bryn, "north"), Tick::new(0), &mut events);
        let mut now = 30;
        while game.character(bryn).unwrap().standing() == Standing::Clean {
            game.apply(&Command::gather(bryn), Tick::new(now), &mut events);
            now += GATHER_TIME.as_ticks();
        }
        while !game.character(bryn).unwrap().is_spirit() {
            game.apply(&Command::attack(ash, bryn), Tick::new(now), &mut events);
            now += ATTACK_TIME.as_ticks();
        }
        // The blow that killed landed one exchange before `now`.
        let died = Tick::new(now - ATTACK_TIME.as_ticks());

        let killer = Scene::of(&game, ash, died).unwrap();
        assert!(killer.company.is_empty(), "the living do not see spirits");
        let here = killer.places.iter().find(|place| place.here).unwrap();
        assert_eq!(here.bodies, ["Bryn"]);
        assert!(here.spoils);
        assert_eq!((killer.you.can_bury.clone(), killer.you.afterlife.clone()), (vec!["Bryn".to_owned()], None));

        // Bryn's spirit was carried off to camp. The scene says what has
        // become of the body, and not where it is.
        let spirit = Scene::of(&game, bryn, died).unwrap();
        assert_eq!(spirit.you.place_name, "Tidewatch Camp");
        assert_eq!(spirit.you.body, Some(BodyView { state: "lying" }));
        assert_eq!(spirit.you.afterlife, Some(AfterlifeView { level: 0, needed: 15 }));
        assert!(spirit.you.can_spar && !spirit.you.can_gather && !killer.you.can_spar, "camp has shades, for spirits");
        assert!(spirit.you.can_bury.is_empty(), "spirits bury nobody");

        let later = died + Duration::minutes(1);
        game.apply(&Command::go(bryn, "north"), died, &mut events);
        let back = Scene::of(&game, bryn, later).unwrap();
        assert!(back.you.can_gather && !back.you.can_spar, "the ledge is somewhere to practise the work");

        // Buried, there is nothing left to take.
        game.apply(&Command::bury(ash, bryn), later, &mut events);
        assert_eq!(Scene::of(&game, bryn, later).unwrap().you.body, Some(BodyView { state: "buried" }));
        let after = Scene::of(&game, ash, later).unwrap();
        assert!(after.you.can_bury.is_empty() && !after.you.can_take);
        assert_eq!(back.company.len(), 1, "spirits see the living");
    }

    #[test]
    fn a_forgotten_spirit_sees_how_far_it_has_to_go() {
        let mut solo = Solo::new(1);
        solo.handle("north");
        solo.handle("sleep 15d");
        let woken = scene(&solo).you.forgotten.unwrap();
        assert_eq!((woken.dormant, woken.memories, woken.needed, woken.can_seek), (false, 0, 3, true));
        assert_eq!(woken.time_left, 36_000);

        assert!(scene(&solo).you.can_gather, "a waking spirit can draw ectoplasm here");
        solo.handle("gather");
        assert_eq!(scene(&solo).you.ectoplasm, 1);
        solo.handle("seek");
        let after = scene(&solo).you.forgotten.unwrap();
        assert_eq!((after.memories, after.can_seek, after.time_left), (1, false, 36_000 - 610));
    }

    /// The same hunt the browser test plays (`web/test/e2e.html`), so a
    /// rule change that would break it shows up here first.
    #[test]
    fn the_browser_tests_hunt_for_the_marked_bot_plays_out() {
        const CIRCUIT: [&str; 14] =
            ["north", "north", "south", "south", "east", "east", "west", "west", "west", "south", "south", "north", "north", "east"];
        let mut solo = Solo::with_company(1, 3);
        solo.handle("wait 10m");
        let can_attack = |solo: &Solo| scene(solo).company.iter().any(|person| person.can_attack);
        let mut step = 0;
        while step < 400 && !can_attack(&solo) {
            solo.handle(CIRCUIT[step % CIRCUIT.len()]);
            solo.handle("wait 20");
            step += 1;
        }
        assert!(can_attack(&solo), "never caught the marked bot in the open");
        let mut said = Vec::new();
        for _ in 0..20 {
            if !can_attack(&solo) {
                break;
            }
            said.extend(solo.handle("attack bryn").lines);
        }
        assert!(said.iter().any(|line| line.starts_with("Bryn dies.")), "{said:?}");
        let after = scene(&solo);
        assert!(after.you.body.is_none(), "the player should come through it alive");
        let here = after.places.iter().find(|place| place.here).unwrap();
        assert_eq!(here.bodies, ["Bryn"]);
        assert!(here.spoils && after.you.can_take, "what Bryn dropped should be there to take: {here:?}");
    }

    #[test]
    fn the_scene_is_the_land_the_viewer_stands_in_and_no_other() {
        use eq_events::World;
        use eq_game::Command;
        let mut game = Game::new_world(3, 2, &[]);
        let (ada, ben) = (CharacterId::new(0), CharacterId::new(1));
        let mut events = Vec::new();
        game.apply(&Command::join(ada, "Ada"), Tick::new(0), &mut events);
        game.apply(&Command::join(ben, "Ben"), Tick::new(0), &mut events);
        game.apply(&Command::name_land(ada, "saltmarch"), Tick::new(1), &mut events);

        let hers = Scene::of(&game, ada, Tick::new(1)).unwrap();
        let his = Scene::of(&game, ben, Tick::new(1)).unwrap();
        assert_eq!((hers.places.len(), his.places.len()), (11, 11));
        assert_eq!((hers.land.as_deref(), his.land.as_deref()), (Some("Saltmarch"), None));
        let ids = |scene: &Scene| scene.places.iter().map(|place| place.id).collect::<Vec<_>>();
        assert!(ids(&hers).iter().all(|id| !ids(&his).contains(id)), "the two maps share no place");
        assert!(hers.links.iter().all(|link| ids(&hers).contains(&link.from) && ids(&hers).contains(&link.to)));
        assert_eq!(hers.links.len(), 10, "eleven places in a tree; the way beyond the pass is not on the map");
        assert!(hers.company.is_empty() && his.company.is_empty());
    }

    #[test]
    fn creatures_and_relics_show_where_they_are() {
        let solo = Solo::new(1);
        let scene = scene(&solo);
        let grotto = scene.places.iter().find(|place| place.name == "Deep Grotto").unwrap();
        let lurker = FoeView { name: "lurker".into(), word: "lurker", health: 60, aggressive: true };
        assert_eq!(grotto.creatures, [lurker]);
        assert!(scene.places.iter().filter(|place| place.name != "Deep Grotto").all(|place| place.creatures.is_empty()));
    }

    #[test]
    fn no_two_places_share_a_spot_and_every_link_is_one_step_long() {
        let scene = scene(&Solo::new(1));
        let spot = |id: u64| scene.places.iter().find(|place| place.id == id).map(|place| (place.x, place.y)).unwrap();

        let mut spots: Vec<(i32, i32)> = scene.places.iter().map(|place| (place.x, place.y)).collect();
        spots.sort_unstable();
        spots.dedup();
        assert_eq!(spots.len(), scene.places.len());

        for link in &scene.links {
            let ((ax, ay), (bx, by)) = (spot(link.from), spot(link.to));
            assert_eq!((ax - bx).abs() + (ay - by).abs(), 1, "link {link:?} is not between neighbours");
        }
    }

    #[test]
    fn exits_follow_you_and_the_tide() {
        let mut solo = Solo::new(1);
        let labels = |scene: &Scene| scene.exits.iter().map(|exit| exit.label.clone()).collect::<Vec<_>>();
        assert_eq!(labels(&scene(&solo)), ["north", "east", "south"]);

        solo.handle("east");
        let at_beach = scene(&solo);
        assert_eq!(labels(&at_beach), ["west", "east", "cave"]);
        assert!(at_beach.exits.iter().all(|exit| exit.open));

        solo.handle("wait 10m");
        let high_tide = scene(&solo);
        assert_eq!(high_tide.tide, "high");
        let cave = high_tide.exits.iter().find(|exit| exit.label == "cave").unwrap();
        assert!(!cave.open);
        assert_eq!(high_tide.links.iter().filter(|link| link.tidal && !link.open).count(), 2);
    }

    #[test]
    fn gathering_shows_in_the_deposit_the_pack_and_the_levels() {
        let mut solo = Solo::new(1);
        solo.handle("north");
        assert!(scene(&solo).you.can_gather);
        for _ in 0..20 {
            solo.handle("gather");
        }
        let scene = scene(&solo);
        let ledge = scene.places.iter().find(|place| place.here).unwrap();
        let deposit = ledge.deposit.as_ref().unwrap();
        assert_eq!(deposit.resource, "heavy ore");
        assert!(deposit.stock_percent < 70);
        assert_ne!(deposit.quality, "pristine");
        assert_eq!(scene.you.pack.len(), 1);
        assert!(scene.you.pack[0].amount > 100);
        assert!(scene.you.skills.iter().any(|skill| skill.name == "mining" && skill.level >= 1));
    }

    #[test]
    fn json_round_trips_the_fields_a_client_reads() {
        let json = scene(&Solo::new(1)).to_json();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["you"]["place_name"], "Tidewatch Camp");
        assert_eq!(value["places"][0]["x"], 2);
        assert_eq!(value["places"][0]["deposit"], serde_json::Value::Null);
        assert_eq!(value["links"][0]["open"], true);
        assert_eq!(value["exits"][0]["label"], "north");
    }

    #[test]
    fn nobody_gets_no_scene() {
        let solo = Solo::new(1);
        assert_eq!(Scene::of(solo.game(), CharacterId::new(9), solo.now()), None);
    }
}
