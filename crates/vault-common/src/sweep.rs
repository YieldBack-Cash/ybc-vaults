use soroban_sdk::{panic_with_error, token::TokenClient, Address, Env};

use crate::{errors::VaultError, events, guard::require_positive};

/// Moves a stray token out of the vault.
///
/// Exists because incentive programs distribute to whichever address held the
/// position, which is the vault, and without a route out anything that lands
/// there is stranded permanently.
///
/// The underlying `asset` and the vault's own share token are hard-refused.
/// Depositor funds are never reachable through this, which is the only reason
/// an admin-held sweep is acceptable at all. **The caller authenticates the
/// admin**; this function only enforces what may be moved.
pub fn sweep(e: &Env, asset: &Address, token: &Address, to: &Address, amount: i128) {
    if token == asset || *token == e.current_contract_address() {
        panic_with_error!(e, VaultError::SweepForbidden);
    }
    require_positive(e, amount);

    TokenClient::new(e, token).transfer(&e.current_contract_address(), to, &amount);
    events::sweep(e, token, to, amount);
}
