//! Events.
//!
//! `receiver`/`owner` are marked `#[topic]`, not left in the data section. An
//! off-chain indexer attributing an incentive program by look-through needs to
//! filter on who ended up holding the shares, and a data-only field cannot be
//! filtered. `blend-vault-v2` topics only the funds-owner, which makes exactly
//! that query impossible against it.

use soroban_sdk::{contractevent, Address, Env};

#[contractevent(topics = ["vault", "deposit"])]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Deposit {
    #[topic]
    pub from: Address,
    #[topic]
    pub receiver: Address,
    /// Measured, not requested — what the vault actually received.
    pub assets: i128,
    pub shares: i128,
}

#[contractevent(topics = ["vault", "redeem"])]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Redeem {
    #[topic]
    pub owner: Address,
    #[topic]
    pub receiver: Address,
    pub shares: i128,
    pub assets: i128,
}

/// Emitted once in the vault's lifetime, when it opens its XOXNO account.
#[contractevent(topics = ["vault", "account_opened"])]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountOpened {
    #[topic]
    pub account_id: u64,
    pub spoke_id: u32,
}

#[contractevent(topics = ["vault", "sweep"])]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Sweep {
    #[topic]
    pub token: Address,
    #[topic]
    pub to: Address,
    pub amount: i128,
}

pub fn deposit(e: &Env, from: &Address, receiver: &Address, assets: i128, shares: i128) {
    Deposit {
        from: from.clone(),
        receiver: receiver.clone(),
        assets,
        shares,
    }
    .publish(e);
}

pub fn redeem(e: &Env, owner: &Address, receiver: &Address, shares: i128, assets: i128) {
    Redeem {
        owner: owner.clone(),
        receiver: receiver.clone(),
        shares,
        assets,
    }
    .publish(e);
}

pub fn account_opened(e: &Env, account_id: u64, spoke_id: u32) {
    AccountOpened {
        account_id,
        spoke_id,
    }
    .publish(e);
}

pub fn sweep(e: &Env, token: &Address, to: &Address, amount: i128) {
    Sweep {
        token: token.clone(),
        to: to.clone(),
        amount,
    }
    .publish(e);
}
