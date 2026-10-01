use soroban_sdk::contracterror;

/// Error codes shared by every adapter.
///
/// # Numbering
///
/// Codes are namespaced so a client can always tell where a failure came from:
///
/// | Range     | Owner                                              |
/// |-----------|----------------------------------------------------|
/// | 10–49     | this enum                                          |
/// | 100–199   | OpenZeppelin `FungibleTokenError` (100–114 in 0.7)  |
/// | 200–299   | `blend-vault`                                      |
/// | 300–399   | `xoxno-vault`                                      |
/// | 400+      | the next adapter, 100 per protocol                 |
///
/// The share token delegates to OpenZeppelin, so its codes surface unchanged
/// from every adapter (`LessThanZero = 103` on a negative transfer, and so on).
/// An adapter must never reuse that range: an adapter code of 102 would be
/// indistinguishable from OZ's `InvalidLiveUntilLedger`.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum VaultError {
    NotInitialized = 10,
    AlreadyInitialized = 11,

    /// An amount at an entry point was zero or negative.
    AmountNotPositive = 20,
    // 40 is retired (was `SweepForbidden`); do not reuse.
}
