//! Ledger movement for tests: advancing time and sequence together, the way
//! the network does, so TTLs and interest accrual see consistent values.

use soroban_sdk::testutils::{Ledger as _, LedgerInfo};
use soroban_sdk::Env;

pub use vault_common::ttl::DAY_IN_LEDGERS as ONE_DAY_LEDGERS;

const PROTOCOL_VERSION: u32 = 26;

pub trait EnvTestUtils {
    /// Jump the env by the given amount of ledgers. Assumes 5 seconds per ledger.
    fn jump(&self, ledgers: u32);

    /// Jump the env by the given amount of seconds. Increments the sequence by 1.
    fn jump_time(&self, seconds: u64);

    /// Set the ledger to the default LedgerInfo
    ///
    /// Time -> 1441065600 (Sept 1st, 2015 12:00:00 AM UTC)
    /// Sequence -> 100
    fn set_default_info(&self);
}

fn info(timestamp: u64, sequence_number: u32) -> LedgerInfo {
    LedgerInfo {
        timestamp,
        protocol_version: PROTOCOL_VERSION,
        sequence_number,
        network_id: Default::default(),
        base_reserve: 10,
        min_temp_entry_ttl: 30 * ONE_DAY_LEDGERS,
        min_persistent_entry_ttl: 30 * ONE_DAY_LEDGERS,
        max_entry_ttl: 365 * ONE_DAY_LEDGERS,
    }
}

impl EnvTestUtils for Env {
    fn jump(&self, ledgers: u32) {
        self.ledger().set(info(
            self.ledger().timestamp().saturating_add(ledgers as u64 * 5),
            self.ledger().sequence().saturating_add(ledgers),
        ));
    }

    fn jump_time(&self, seconds: u64) {
        self.ledger().set(info(
            self.ledger().timestamp().saturating_add(seconds),
            self.ledger().sequence().saturating_add(1),
        ));
    }

    fn set_default_info(&self) {
        self.ledger().set(info(1441065600, 100));
    }
}
