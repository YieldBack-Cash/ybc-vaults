use soroban_sdk::Env;

/// Ledgers per day at the 5-second ledger close (86_400 / 5).
pub const DAY_IN_LEDGERS: u32 = 17280;
/// The instance is bumped thirty days ahead once fewer than twenty-nine
/// remain: it holds the vault's configuration and totals, and an expired
/// instance halts the vault until someone pays to restore it. Adapters may
/// bump their own persistent entries on a longer cycle.
pub const INSTANCE_BUMP_AMOUNT: u32 = 30 * DAY_IN_LEDGERS;
pub const INSTANCE_LIFETIME_THRESHOLD: u32 = INSTANCE_BUMP_AMOUNT - DAY_IN_LEDGERS;

/// Extends the instance TTL. Called on every state-changing entry point.
///
/// Balances and allowances are OpenZeppelin's, which bumps them itself.
pub fn extend_instance_ttl(e: &Env) {
    e.storage()
        .instance()
        .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
}
