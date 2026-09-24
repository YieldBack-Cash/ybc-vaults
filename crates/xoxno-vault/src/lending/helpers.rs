//! Authorization helpers for contracts that call XOXNO Lending. Verbatim from
//! `xoxno_contract_sdk::lending::helpers` 0.1.0, minus the flash-loan helper
//! this vault has no use for.

use soroban_sdk::auth::{ContractContext, InvokerContractAuthEntry, SubContractInvocation};
use soroban_sdk::{symbol_short, vec, Address, Env, IntoVal, Vec};

/// Authorizes one `transfer(from, to, amount)` on `token` inside the next call
/// the current contract makes.
///
/// Use it before `supply` or `repay` when the current contract is the payer:
/// the controller moves the tokens with `transfer`, which is a sub-invocation
/// of the controller call and therefore needs this entry.
pub fn authorize_transfer_as_current(
    env: &Env,
    token: &Address,
    from: &Address,
    to: &Address,
    amount: i128,
) {
    env.authorize_as_current_contract(vec![
        env,
        InvokerContractAuthEntry::Contract(SubContractInvocation {
            context: ContractContext {
                contract: token.clone(),
                fn_name: symbol_short!("transfer"),
                args: (from.clone(), to.clone(), amount).into_val(env),
            },
            sub_invocations: Vec::new(env),
        }),
    ]);
}
