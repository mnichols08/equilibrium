//! Factions: people of several bloodlines sworn to one charter.
//!
//! A charter is opened at a house, which is its keep. It becomes a
//! faction once enough people of enough bloodlines have signed it. A
//! faction keeps a vault, and holds ground by setting markers that the
//! vault must pay for every day. It ends when the vault cannot pay, when
//! one of those who first signed it breaks their oath, or when the
//! marker at its keep is broken.
//!
//! Nothing here moves anyone, pays anyone or knows the map: the rules
//! in `game` do that, and ask this who is sworn to what.

use alloc::string::String;
use alloc::vec::Vec;

use eq_clock::Tick;
use eq_world_graph::PlaceId;

use crate::bloodline::Bloodline;
use crate::character::CharacterId;
use crate::content::{CHARTER_BLOODLINES, CHARTER_SIGNATORIES};
use crate::types::Resource;

/// Whose a relic spilt from a broken vault is: nobody's. Anyone may
/// open it.
pub const NOBODY: CharacterId = CharacterId::new(u64::MAX);

/// Which faction, for as long as it lasts. Never reused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FactionId(u64);

impl FactionId {
    /// The id with this number.
    pub const fn new(raw: u64) -> Self {
        Self(raw)
    }

    /// The number.
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// A place a faction holds, by the marker set there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Claim {
    /// Where the marker stands.
    pub place: PlaceId,
    /// Someone at work breaking the marker, and when it will give.
    pub breaking: Option<(CharacterId, Tick)>,
}

/// One charter, signed or still being signed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Faction {
    /// Which faction.
    pub id: FactionId,
    /// What it is called. For good.
    pub name: String,
    /// The house it was opened at. Its first marker stands here.
    pub keep: PlaceId,
    /// Who signed before it was formed, and of what blood, in order.
    pub signatories: Vec<(CharacterId, Bloodline)>,
    /// Everyone sworn to it, the signatories first.
    pub members: Vec<CharacterId>,
    /// When it was formed, once it has been.
    pub formed_at: Option<Tick>,
    /// What it holds in common, in pack order.
    pub vault: [u64; Resource::ALL.len()],
    /// The ground it holds, the keep first.
    pub claims: Vec<Claim>,
    /// When the vault next pays for the markers.
    pub upkeep_due: Tick,
}

impl Faction {
    /// Whether enough people of enough bloodlines have signed.
    pub fn is_formed(&self) -> bool {
        self.formed_at.is_some()
    }

    /// Whether `who` is sworn to it.
    pub fn has(&self, who: CharacterId) -> bool {
        self.members.contains(&who)
    }

    /// Whether `who` signed it before it was formed.
    pub fn signed_first(&self, who: CharacterId) -> bool {
        self.signatories.iter().any(|&(signed, _)| signed == who)
    }

    /// How many different bloodlines have signed.
    pub fn bloodlines(&self) -> usize {
        let mut seen: Vec<Bloodline> = Vec::new();
        for &(_, blood) in &self.signatories {
            if !seen.contains(&blood) {
                seen.push(blood);
            }
        }
        seen.len()
    }

    /// Whether the signatures are enough to form it.
    pub fn is_ready(&self) -> bool {
        self.signatories.len() >= CHARTER_SIGNATORIES && self.bloodlines() >= CHARTER_BLOODLINES
    }

    /// Whether it holds `place`.
    pub fn holds(&self, place: PlaceId) -> bool {
        self.claims.iter().any(|claim| claim.place == place)
    }

    /// How much of `resource` the vault holds.
    pub const fn held(&self, resource: Resource) -> u64 {
        self.vault[resource.index()]
    }
}

/// How a faction came to an end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dissolution {
    /// The vault could not pay for its markers.
    Exhausted,
    /// One of those who first signed it broke their oath.
    Betrayed(CharacterId),
    /// The marker at its keep was broken, by this person.
    Broken(CharacterId),
}

/// Why a charter, an oath or a marker could not be dealt with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FactionRefusal {
    /// A charter is opened in a house, and nowhere else.
    NotAtAHouse,
    /// That is done at the faction's keep.
    NotAtKeep,
    /// There is no charter of that name.
    NoSuchCharter,
    /// They are sworn to a faction, or have signed a charter, already.
    Sworn,
    /// They broke an oath, and nobody will take theirs yet.
    Oathbreaker,
    /// The name will not do.
    Name(eq_naming::Problem),
    /// They are sworn to no faction.
    NotSworn,
    /// The charter has not been signed by enough yet.
    NotFormed,
    /// Markers are not set in a haven, a lair or someone's house.
    CannotHoldHere,
    /// A faction holds this ground already.
    Held,
    /// The vault cannot pay for a marker.
    VaultShort,
    /// They are not carrying that much.
    NothingToGive,
    /// No faction holds this ground.
    NothingToBreak,
    /// It is their own faction's marker.
    OwnMarker,
    /// Someone is breaking this marker already.
    AlreadyBreaking,
}

/// What a faction's members hear of it, wherever they are.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FactionNews {
    /// Enough have signed: it is a faction now.
    Formed,
    /// Someone swore to it.
    Joined(CharacterId),
    /// Someone broke their oath to it and left.
    Left(CharacterId),
    /// A marker was set here.
    Claimed(PlaceId),
    /// Someone not sworn to it came onto ground it holds.
    Trespass {
        /// Who.
        who: CharacterId,
        /// Where.
        place: PlaceId,
    },
    /// Someone set to breaking a marker.
    Breaking {
        /// Who.
        who: CharacterId,
        /// Where.
        place: PlaceId,
    },
    /// A marker was broken, and the ground is no longer held.
    MarkerBroken {
        /// Who broke it.
        by: CharacterId,
        /// Where.
        place: PlaceId,
    },
    /// The vault paid for the markers.
    UpkeepPaid {
        /// How much it paid.
        amount: u64,
    },
    /// It is over.
    Dissolved(Dissolution),
}

/// Someone who broke an oath, or was sworn to a faction that failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Oathbreaker {
    /// Who.
    pub who: CharacterId,
    /// Until when nobody will take their oath.
    pub until: Tick,
    /// Those they turned their back on, who may strike them for it.
    /// Nobody, if the faction failed them rather than they it.
    pub wronged: Vec<CharacterId>,
}

/// Every faction and every charter still being signed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Factions {
    all: Vec<Faction>,
    oathbreakers: Vec<Oathbreaker>,
    next: u64,
}

impl Factions {
    /// None at all.
    pub const fn new() -> Self {
        Self { all: Vec::new(), oathbreakers: Vec::new(), next: 0 }
    }

    /// Every faction and charter, oldest first.
    pub fn all(&self) -> &[Faction] {
        &self.all
    }

    /// The faction or charter with this id.
    pub fn get(&self, id: FactionId) -> Option<&Faction> {
        self.all.iter().find(|faction| faction.id == id)
    }

    pub(crate) fn get_mut(&mut self, id: FactionId) -> Option<&mut Faction> {
        self.all.iter_mut().find(|faction| faction.id == id)
    }

    /// What `who` is sworn to or has signed, if anything.
    pub fn of(&self, who: CharacterId) -> Option<&Faction> {
        self.all.iter().find(|faction| faction.has(who))
    }

    /// The faction or charter called `name`, whatever its capitals.
    pub fn named(&self, name: &str) -> Option<&Faction> {
        self.all.iter().find(|faction| faction.name.eq_ignore_ascii_case(name))
    }

    /// The faction that holds `place`, if any.
    pub fn holding(&self, place: PlaceId) -> Option<&Faction> {
        self.all.iter().find(|faction| faction.holds(place))
    }

    /// Whether `place` is held by a faction `who` is sworn to.
    pub fn holds_for(&self, who: CharacterId, place: PlaceId) -> bool {
        self.holding(place).is_some_and(|faction| faction.has(who))
    }

    /// Whether `who` is an oathbreaker. Their time is up once
    /// [`Factions::forgive`] has been called past it.
    pub fn oathbreaker(&self, who: CharacterId) -> Option<&Oathbreaker> {
        self.oathbreakers.iter().find(|broken| broken.who == who)
    }

    /// Whether `by` may strike `traitor` for turning their back on them.
    pub fn wronged(&self, traitor: CharacterId, by: CharacterId) -> bool {
        self.oathbreaker(traitor).is_some_and(|broken| broken.wronged.contains(&by))
    }

    /// Lets go of every oathbreaker whose time is up at `now`.
    pub(crate) fn forgive(&mut self, now: Tick) {
        self.oathbreakers.retain(|broken| now < broken.until);
    }

    /// Opens a charter called `name` at `keep`, signed first by `by`.
    pub(crate) fn open(&mut self, name: String, keep: PlaceId, by: CharacterId, blood: Bloodline, now: Tick) -> FactionId {
        let id = FactionId(self.next);
        self.next += 1;
        self.all.push(Faction {
            id,
            name,
            keep,
            signatories: alloc::vec![(by, blood)],
            members: alloc::vec![by],
            formed_at: None,
            vault: [0; Resource::ALL.len()],
            claims: Vec::new(),
            upkeep_due: now,
        });
        id
    }

    /// `who` signs. Before the faction is formed they are a signatory;
    /// after, a member. Returns whether this signature formed it.
    pub(crate) fn sign(&mut self, id: FactionId, who: CharacterId, blood: Bloodline, now: Tick, upkeep_every: eq_clock::Duration) -> bool {
        let Some(faction) = self.get_mut(id) else { return false };
        faction.members.push(who);
        if faction.is_formed() {
            return false;
        }
        faction.signatories.push((who, blood));
        if !faction.is_ready() {
            return false;
        }
        faction.formed_at = Some(now);
        faction.upkeep_due = now + upkeep_every;
        let keep = faction.keep;
        faction.claims.push(Claim { place: keep, breaking: None });
        true
    }

    /// `who` leaves. Returns the faction as it stood, if they were in one.
    pub(crate) fn leave(&mut self, who: CharacterId) -> Option<Faction> {
        let slot = self.all.iter().position(|faction| faction.has(who))?;
        let before = self.all[slot].clone();
        let faction = &mut self.all[slot];
        faction.members.retain(|&member| member != who);
        if !faction.is_formed() {
            faction.signatories.retain(|&(signed, _)| signed != who);
        }
        if faction.members.is_empty() {
            self.all.remove(slot);
        }
        Some(before)
    }

    /// Ends a faction and returns what it was.
    pub(crate) fn dissolve(&mut self, id: FactionId) -> Option<Faction> {
        let slot = self.all.iter().position(|faction| faction.id == id)?;
        Some(self.all.remove(slot))
    }

    /// Marks `who` an oathbreaker until `until`.
    pub(crate) fn break_oath(&mut self, who: CharacterId, until: Tick, wronged: Vec<CharacterId>) {
        self.oathbreakers.retain(|broken| broken.who != who);
        self.oathbreakers.push(Oathbreaker { who, until, wronged });
    }

    /// Every claim being broken, with whose faction it is.
    pub(crate) fn breakings(&self) -> Vec<(FactionId, PlaceId, CharacterId, Tick)> {
        let claims = self.all.iter().flat_map(|faction| faction.claims.iter().map(move |claim| (faction.id, claim)));
        claims.filter_map(|(id, claim)| claim.breaking.map(|(who, at)| (id, claim.place, who, at))).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eq_clock::Duration;

    const DAY: Duration = Duration::days(1);

    fn someone(n: u64) -> CharacterId {
        CharacterId::new(n)
    }

    #[test]
    fn a_charter_forms_only_with_enough_people_of_enough_bloodlines() {
        let mut factions = Factions::new();
        let keep = PlaceId::new(3);
        let id = factions.open("Saltward".into(), keep, someone(0), Bloodline::ALL[0], Tick::ZERO);
        // Five of only two bloodlines are not enough.
        for n in 1..5 {
            assert!(!factions.sign(id, someone(n), Bloodline::ALL[(n % 2) as usize], Tick::ZERO, DAY));
        }
        assert!(!factions.get(id).unwrap().is_formed());
        // A sixth, of a third blood, forms it.
        assert!(factions.sign(id, someone(5), Bloodline::ALL[2], Tick::new(9), DAY));
        let formed = factions.get(id).unwrap();
        assert!(formed.is_formed() && formed.holds(keep));
        assert_eq!(formed.upkeep_due, Tick::new(9) + DAY);
        // After that, signing only swears someone in.
        assert!(!factions.sign(id, someone(6), Bloodline::ALL[0], Tick::new(10), DAY));
        let formed = factions.get(id).unwrap();
        assert_eq!((formed.members.len(), formed.signatories.len()), (7, 6));
        assert!(formed.has(someone(6)) && !formed.signed_first(someone(6)));
        assert_eq!(factions.holding(keep).map(|faction| faction.id), Some(id));
        assert!(factions.holds_for(someone(6), keep) && !factions.holds_for(someone(7), keep));
    }

    #[test]
    fn leaving_an_unformed_charter_takes_back_the_signature_and_the_last_to_leave_ends_it() {
        let mut factions = Factions::new();
        let id = factions.open("Saltward".into(), PlaceId::new(3), someone(0), Bloodline::ALL[0], Tick::ZERO);
        factions.sign(id, someone(1), Bloodline::ALL[1], Tick::ZERO, DAY);
        factions.leave(someone(0));
        assert_eq!(factions.get(id).unwrap().signatories, [(someone(1), Bloodline::ALL[1])]);
        factions.leave(someone(1));
        assert!(factions.get(id).is_none());
        assert!(factions.named("SALTWARD").is_none());
    }

    #[test]
    fn an_oathbreaker_is_one_for_a_while_and_only_those_wronged_may_strike() {
        let mut factions = Factions::new();
        factions.break_oath(someone(0), Tick::new(100), alloc::vec![someone(1)]);
        assert!(factions.wronged(someone(0), someone(1)));
        assert!(!factions.wronged(someone(0), someone(2)));
        factions.forgive(Tick::new(99));
        assert!(factions.oathbreaker(someone(0)).is_some());
        factions.forgive(Tick::new(100));
        assert!(factions.oathbreaker(someone(0)).is_none());
    }
}
