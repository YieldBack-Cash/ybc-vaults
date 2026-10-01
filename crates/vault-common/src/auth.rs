use soroban_sdk::auth::{ContractContext, InvokerContractAuthEntry, SubContractInvocation};
use soroban_sdk::{symbol_short, vec, Address, Env, IntoVal, Vec};
use stellar_tokens::fungible::Base;

/// Authorizes one `transfer(from, to, amount)` on `token` inside the very
/// next call the current contract makes, and no other.
///
/// Both lending protocols and the Soroswap router pull tokens from this
/// contract with a `transfer` nested inside their own entry point, after
/// `from.require_auth()`. A contract authorizes such a nested call with
/// `authorize_as_current_contract`, naming the exact call; an allowance would
/// cover only `transfer_from`, which none of them use. The grant is consumed
/// by the next contract call whatever it is, so nothing may sit between this
/// and the call it is meant for: even a balance read would take it.
pub fn authorize_transfer_as_current(
    e: &Env,
    token: &Address,
    from: &Address,
    to: &Address,
    amount: i128,
) {
    e.authorize_as_current_contract(vec![
        e,
        InvokerContractAuthEntry::Contract(SubContractInvocation {
            context: ContractContext {
                contract: token.clone(),
                fn_name: symbol_short!("transfer"),
                args: (from.clone(), to.clone(), amount).into_val(e),
            },
            sub_invocations: Vec::new(e),
        }),
    ]);
}

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
