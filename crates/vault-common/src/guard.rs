use soroban_sdk::{panic_with_error, Env};

use crate::errors::VaultError;

/// Refuses a zero or negative amount with [`VaultError::AmountNotPositive`].
///
/// Every value-moving entry point calls this before touching auth or storage.
/// The threat model's critical findings F-1 and F-3 were both an entry point
/// that skipped exactly this check and let a negative amount turn a debit into
/// a credit.
pub fn require_positive(e: &Env, amount: i128) {
    if amount <= 0 {
        panic_with_error!(e, VaultError::AmountNotPositive);
    }
}
