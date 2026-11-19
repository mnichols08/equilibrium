//! Escorts: someone pays to be seen safely to a place.
//!
//! A client puts up a reward and names where they are going. A guard
//! takes the job. The reward is the guard's when both stand at that
//! place alive. Nothing here moves anyone or pays anyone: the rules in
//! `game` do that, and ask this what is owed.

use alloc::vec::Vec;

use eq_world_graph::PlaceId;

use crate::board::Lot;
use crate::character::CharacterId;

/// One escort: who is going where, for what, and who is seeing them there.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Escort {
    /// Who is paying to be seen safely there.
    pub client: CharacterId,
    /// Who has taken the job, once someone has.
    pub guard: Option<CharacterId>,
    /// What it pays.
    pub reward: Lot,
    /// Where the client is going.
    pub to: PlaceId,
    /// The haven whose board the job was put up on, and where the
    /// reward goes back to if it comes to nothing.
    pub posted_at: PlaceId,
}

/// Every escort that is wanted or under way.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Escorts {
    jobs: Vec<Escort>,
}

impl Escorts {
    /// No escorts.
    pub const fn new() -> Self {
        Self { jobs: Vec::new() }
    }

    /// Every escort, oldest first.
    pub fn all(&self) -> &[Escort] {
        &self.jobs
    }

    /// The escort `client` has asked for, if any.
    pub fn of(&self, client: CharacterId) -> Option<&Escort> {
        self.jobs.iter().find(|job| job.client == client)
    }

    /// Puts up a job. Returns `false`, and changes nothing, if the
    /// client has one up already.
    pub(crate) fn open(&mut self, job: Escort) -> bool {
        let free = self.of(job.client).is_none();
        if free {
            self.jobs.push(job);
        }
        free
    }

    /// `guard` takes `client`'s job. Returns `false`, and changes
    /// nothing, if there is no such job, it is taken, or it is the
    /// guard's own.
    pub(crate) fn take(&mut self, client: CharacterId, guard: CharacterId) -> bool {
        match self.jobs.iter_mut().find(|job| job.client == client && job.guard.is_none() && client != guard) {
            Some(job) => {
                job.guard = Some(guard);
                true
            }
            None => false,
        }
    }

    /// Ends `client`'s job, whatever state it is in, and returns it.
    pub(crate) fn close(&mut self, client: CharacterId) -> Option<Escort> {
        let slot = self.jobs.iter().position(|job| job.client == client)?;
        Some(self.jobs.remove(slot))
    }

    /// Frees every job `guard` had taken, so another can take it.
    /// Returns the clients left without a guard.
    pub(crate) fn release(&mut self, guard: CharacterId) -> Vec<CharacterId> {
        let left: Vec<&mut Escort> = self.jobs.iter_mut().filter(|job| job.guard == Some(guard)).collect();
        left.into_iter()
            .map(|job| {
                job.guard = None;
                job.client
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Resource;

    const ASH: CharacterId = CharacterId::new(0);
    const BRYN: CharacterId = CharacterId::new(1);
    const COLE: CharacterId = CharacterId::new(2);

    fn job(client: CharacterId) -> Escort {
        let reward = Lot { resource: Resource::Ore, amount: 20 };
        Escort { client, guard: None, reward, to: PlaceId::new(9), posted_at: PlaceId::new(0) }
    }

    #[test]
    fn one_job_each_and_one_guard_a_job() {
        let mut escorts = Escorts::new();
        assert!(escorts.open(job(ASH)));
        assert!(!escorts.open(job(ASH)), "one at a time");
        assert!(!escorts.take(ASH, ASH), "nobody guards themselves");
        assert!(!escorts.take(BRYN, COLE), "Bryn asked for nobody");
        assert!(escorts.take(ASH, BRYN));
        assert!(!escorts.take(ASH, COLE), "it is taken");
        assert_eq!(escorts.of(ASH).and_then(|job| job.guard), Some(BRYN));
    }

    #[test]
    fn a_guard_who_falls_leaves_the_job_open_and_a_closed_job_is_gone() {
        let mut escorts = Escorts::new();
        escorts.open(job(ASH));
        escorts.take(ASH, BRYN);
        assert_eq!(escorts.release(BRYN), [ASH]);
        assert!(escorts.take(ASH, COLE));
        assert_eq!(escorts.close(ASH).map(|job| job.guard), Some(Some(COLE)));
        assert!(escorts.all().is_empty() && escorts.close(ASH).is_none());
    }
}
