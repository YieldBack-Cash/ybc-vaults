# xoxno-vault

A SEP-56 tokenized vault over a [XOXNO](https://xoxno.com/docs/stellar-lending/overview)
lending position on Stellar Soroban, built to back YBC PT/YT markets.

## The idea in one paragraph

XOXNO stores a supply position as a `scaled_amount` — a figure that does **not**
change as interest accrues — against a per-market `supply_index` in RAY. Your
real balance is `scaled_amount × supply_index / RAY`. That is already the
tokenized-vault share primitive: a fixed claim on a growing pool. So this vault
does not invent a second one. **Shares are 1:1 with XOXNO scaled units, and the
exchange rate is the market's supply index.**

This is the same choice Pendle makes wrapping Aave: `PendleAaveV3SY` mirrors
Aave's scaled balance against the liquidity index rather than running its own
share ratio.

## The invariant

```
total_supply  <=  the vault's XOXNO scaled position
```

Deposits mint exactly the scaled delta XOXNO credited, so the two track exactly.
Redeems burn exactly the shares requested while XOXNO's ceil-rounded scaled burn
is never more than that, so dust accrues to remaining holders and the vault only
ever becomes *more* over-collateralized. Never the other way round — which is why
the relation is `<=` and not `=`.

## What that buys

| Property | Why |
|---|---|
| **No inflation guard** | Share price comes from the market index, not from any balance this contract holds. A donation into the vault's XOXNO account raises the position without raising `total_supply` — it widens over-collateralization instead of inflating price. No virtual offset, no dead shares. |
| **No bootstrap deposit** | `convert_to_assets(1e7)` is positive on an empty vault, so a consumer probing the rate at market creation succeeds. A ratio-based vault divides by zero here. |
| **No rounding wedge** | Price moves only when XOXNO's index moves, so a consumer that ratchets its rate engages only on real write-downs, never on dust. |

## Surface

Four functions are what YBC actually calls:

```rust
query_asset() -> Address
convert_to_assets(shares: i128) -> i128
deposit(assets: i128, receiver: Address, from: Address, operator: Address) -> i128
redeem(shares: i128, receiver: Address, owner: Address, operator: Address) -> i128
```

Plus the full SEP-41 token surface on the same address (consumers custody these
shares and hold them as an AMM reserve), and three extras: `total_assets`,
`max_deposit`, `max_withdraw`.

`sweep` is the only privileged function. It moves a stray token — an airdrop that
landed on the vault's address — to a configured destination, and **hard-refuses
the underlying asset and the vault's own share token**. Depositor funds are not
reachable through it, which is the only reason an admin-held sweep is acceptable.

## Deployment

```bash
stellar contract deploy --wasm target/wasm32v1-none/release/xoxno_vault.wasm \
  -- --controller <CONTROLLER> --asset <USDC_SAC> --admin <ADMIN> \
     --hub-id 1 --spoke-id 1 --name "XOXNO USDC Vault" --symbol xvUSDC
```

`hub_id` and `spoke_id` are constructor parameters rather than constants: an
account's spoke binding is permanent, so baking the choice into the WASM would
make it unrecoverable without a new binary. The constructor probes
`get_market_index` so a wrong `hub_id` fails at deployment rather than at the
first deposit.

Mainnet controller: `CAUCMIN5KSXEVZ7NMXR3LZATGD5EFIEUI5XWTFLYRO2R5OTXI22WE5JX`.
Hub 1 USDC on spoke 1 has a **5,000,000 supply cap**, which is the ceiling on
total vault size.

## Build and test

This crate is a member of the `ybc-vaults` workspace; build and test from the
workspace root:

```bash
make test    # cargo test --workspace: this crate's tests plus the shared conformance suite
make build   # stellar contract build --optimize
```

`stellar contract build` is required rather than `cargo build --target
wasm32v1-none` — soroban-sdk 26's `experimental_spec_shaking_v2` feature needs
the CLI wrapper (v25.2.0+), and a bare cargo build fails in the SDK's build
script.

## What is shared and what is XOXNO's

The share token (OpenZeppelin `Base`), the operator-allowance rule on `redeem`,
the positive-amount guard, `sweep`, the TTL policy, the `Deposit`/`Redeem`/
`Sweep` events and the widening multiply come from `vault-common`. The
`tests/conformance.rs` file binds this crate's fixture to `vault-testkit`, which
runs the same 22 properties against every adapter.

This crate owns only what is XOXNO's: the hand-written controller client
(`controller.rs`), the account-id sentinel, the index maths (`vault.rs`), the
`max_*`/`total_assets` views, and the two XOXNO-specific errors
(`ZeroAssetRedeem = 300`, `NoAccount = 301`; shared codes are 10–49,
OpenZeppelin's are 100–199).

## Notes for anyone extending this

**`soroban-fixed-point-math` is deliberately absent.** Its 1.5.0 release pins
soroban-sdk 25.3.2, so its `Env` is a distinct type from ours and every
env-taking helper fails to typecheck. The widening multiply lives in
`vault_common::math::mul_div_floor` on the SDK's own `U256`. The widening is not
optional: at the 5M supply cap `shares × index` is around 5e40 against an
`i128` ceiling near 1.7e38.

**The zero sentinel.** XOXNO reads a withdrawal amount of `0` as *withdraw
everything from this market*. A dust redeem flooring to zero assets would hand
the entire pooled position to whoever asked for one share. `redeem` refuses it —
see `tests/hazards.rs`.

**Deposits measure, never compute.** XOXNO's own rounding decides how many scaled
units a supply credited. A locally computed guess that floored differently would
break the invariant cumulatively rather than once.

**The account id is never cleared.** XOXNO's reference adapter clears it when the
controller reports the account gone; that is deliberately not ported. The id is
the only route back to the collateral and nothing can re-point it.

**There is no harvest.** XOXNO has no claimable on-chain rewards — the
controller's only related function is `claim_revenue`, which sweeps XOXNO's own
protocol revenue to its treasury. Its lending airdrop is an off-chain leaderboard
scored per address, in the same category as EigenLayer/Ethena points. Pendle
writes no reward code for that category either; the points issuer maintains
adapters that read on-chain state and attribute by look-through. That is why the
events here mark holder addresses as topics: an indexer needs to filter on who
ended up holding the shares.

## Status

Pre-audit. XOXNO itself is also pre-audit (external review by Runtime
Verification and Certora pending), and any market built on this vault inherits
that.

`no_seize` is `false` for hub 1 USDC on spoke 1, so the position is fully exposed
to bad-debt socialization: a `seize_positions` write-down lowers the supply index
and therefore the share price. The vault reports that honestly. A consumer with a
high-water-marked rate will ignore the fall by design, which leaves its
fixed-rate leg over-valued against real backing — a property of the consumer, not
of this vault, and not detectable on-chain.
