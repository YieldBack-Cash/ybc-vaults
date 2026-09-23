use soroban_sdk::{Address, Env};
use stellar_tokens::fungible::Base;

/// Authorizes `operator` to burn `shares` of `owner`'s position.
///
/// The caller has already established that `operator` authorized the call.
/// When `operator` is not `owner`, it must additionally hold a share allowance
/// from `owner` covering `shares`, which is consumed here: the same delegation
/// `transfer_from` uses, and the ERC-4626 / SEP-56 rule for a third-party
/// operator. Without it, `operator` would be the only authenticated party in
/// the call and any address could name an arbitrary `owner`.
///
/// Note the early return rather than an unconditional `owner.require_auth()`:
/// a second `require_auth` on an address already authorized in the same frame
/// is a host error (`Auth, ExistingValue`), not a no-op.
pub fn spend_operator_allowance(e: &Env, owner: &Address, operator: &Address, shares: i128) {
    if operator == owner {
        return;
    }
    Base::spend_allowance(e, owner, operator, shares);
}
