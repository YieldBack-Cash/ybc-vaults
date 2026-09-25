//! The two SEP-56 events, exactly as the standard defines them: the same
//! struct names, the same fields in the same order, `operator`, `from`,
//! `receiver` and `owner` as topics, and the default topic name (the struct
//! name in snake case), so an indexer written against the standard sees these
//! vaults without special cases.
//!
//! `mint` publishes `Deposit` and `withdraw` publishes `Withdraw`, as the
//! standard says: the event describes the flow of assets and shares, not the
//! entry point that caused it.
//!
//! Adapters add their own protocol-specific events beside these.

use soroban_sdk::{contractevent, Address, Env};

/// Assets went in, shares came out.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Deposit {
    /// The address that initiated the deposit transaction.
    #[topic]
    pub operator: Address,
    /// The address that provided the underlying assets.
    #[topic]
    pub from: Address,
    /// The address that received the vault shares.
    #[topic]
    pub receiver: Address,
    /// Measured, not requested: what the vault actually received.
    pub assets: i128,
    pub shares: i128,
}

/// Shares were burned, assets went out.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Withdraw {
    /// The address that initiated the withdrawal transaction.
    #[topic]
    pub operator: Address,
    /// The address that received the underlying assets.
    #[topic]
    pub receiver: Address,
    /// The address whose vault shares were burned.
    #[topic]
    pub owner: Address,
    pub assets: i128,
    pub shares: i128,
}

pub fn deposit(
    e: &Env,
    operator: &Address,
    from: &Address,
    receiver: &Address,
    assets: i128,
    shares: i128,
) {
    Deposit {
        operator: operator.clone(),
        from: from.clone(),
        receiver: receiver.clone(),
        assets,
        shares,
    }
    .publish(e);
}

pub fn withdraw(
    e: &Env,
    operator: &Address,
    receiver: &Address,
    owner: &Address,
    assets: i128,
    shares: i128,
) {
    Withdraw {
        operator: operator.clone(),
        receiver: receiver.clone(),
        owner: owner.clone(),
        assets,
        shares,
    }
    .publish(e);
}
