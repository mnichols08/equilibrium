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
use eq_game::content::{LOW_TIDE, TIDE};
use eq_game::{narrate, pressure_percent, Attribute, CharacterId, Deposit, Game, Resource, SkillKind, Standing};
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
    /// Zone vitality, 0 to 100.
    pub vitality: i64,
    /// The zone's vitality band, as shown to players.
    pub health: &'static str,
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
    /// Whether the viewer is here.
    pub here: bool,
    /// What can be gathered here, if anything.
    pub deposit: Option<DepositView>,
    /// Who is here.
    pub people: Vec<PersonView>,
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

/// Someone standing in a place.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PersonView {
    /// Their name.
    pub name: String,
    /// Whether this is the viewer.
    pub you: bool,
    /// Whether the land has marked them.
    pub marked: bool,
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
            can_gather: place.deposit.is_some_and(|deposit| is_workable(&deposit)),
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
            vitality: game.vitality(),
            health: game.health().name(),
            you,
            company: people(game, viewer, here).into_iter().filter(|person| !person.you).collect(),
            places: places(game, viewer, here),
            links: links(game, now),
            exits: exits(game, here, now),
        })
    }

    /// The scene as JSON.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("a scene holds only plain data")
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

fn places(game: &Game, viewer: CharacterId, here: PlaceId) -> Vec<PlaceView> {
    game.map()
        .places()
        .map(|(id, place)| PlaceView {
            id: id.get(),
            name: place.name,
            x: place.position.0,
            y: place.position.1,
            safety: place.safety.name(),
            here: id == here,
            deposit: place.deposit.as_ref().map(deposit_view),
            people: people(game, viewer, id),
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

fn people(game: &Game, viewer: CharacterId, place: PlaceId) -> Vec<PersonView> {
    game.occupants(place)
        .iter()
        .filter_map(|&id| Some((id, game.character(id)?)))
        .map(|(id, character)| PersonView {
            name: character.name().to_owned(),
            you: id == viewer,
            marked: character.standing() == Standing::Marked,
        })
        .collect()
}

/// Each two-way path once, taken from its lower-numbered end.
fn links(game: &Game, now: Tick) -> Vec<LinkView> {
    game.map()
        .all_exits()
        .filter(|(from, exit)| *from < exit.to)
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
        assert_eq!(here[0].people, [PersonView { name: "You".into(), you: true, marked: false }]);
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
