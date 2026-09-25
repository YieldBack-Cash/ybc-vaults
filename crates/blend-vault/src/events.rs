//! Blend-specific events. `Deposit` and `Redeem` are the shared ones
//! in `vault_common::events`; the SEP-41 events are OpenZeppelin's.

use soroban_sdk::{contractevent, Address, Env};

/// Emitted when BLND emissions are claimed, swapped, and supplied back into
/// the pool on behalf of every share holder.
#[contractevent(topics = ["vault", "emissions_claim"])]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EmissionsClaim {
    #[topic]
    pub pool: Address,
    pub blnd_claimed: i128,
    pub underlying_received: i128,
}

pub fn emissions_claim(e: &Env, pool: &Address, blnd_claimed: i128, underlying_received: i128) {
    EmissionsClaim {
        pool: pool.clone(),
        blnd_claimed,
        underlying_received,
    }
    .publish(e);
}
