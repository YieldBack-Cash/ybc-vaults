//! XOXNO-specific events. `Deposit`, `Redeem` and `Sweep` are the shared ones
//! in `vault_common::events`.

use soroban_sdk::{contractevent, Env};

/// Emitted once in the vault's lifetime, when it opens its XOXNO account.
#[contractevent(topics = ["vault", "account_opened"])]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountOpened {
    #[topic]
    pub account_id: u64,
    pub spoke_id: u32,
}

pub fn account_opened(e: &Env, account_id: u64, spoke_id: u32) {
    AccountOpened {
        account_id,
        spoke_id,
    }
    .publish(e);
}
