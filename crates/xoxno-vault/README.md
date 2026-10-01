# xoxno-vault

A SEP-56 tokenized vault over a [XOXNO](https://xoxno.com/docs/stellar-lending/overview)
lending position on Stellar Soroban, built to back YBC PT/YT markets.

## The idea in one paragraph

XOXNO stores a supply position as a `scaled_amount` — a figure that does **not**
change as interest accrues — against a per-market `supply_index` in RAY. Your
real balance is `scaled_amount × supply_index / RAY`. That is already the
tokenized-vault share primitive: a fixed claim on a growing pool. So this vault
does not invent a second one. **Shares are XOXNO's scaled units at asset
precision, and the exchange rate is the market's supply index.** XOXNO stores
`scaled_amount` as a 27-decimal Ray (`from_asset(amount) / index`), so for a
7-decimal asset one share is `10^20` of those raw units (`vault::SCALED_UNIT`).
The constructor refuses an asset whose `decimals()` is not 7.

This is the same choice Pendle makes wrapping Aave: `PendleAaveV3SY` mirrors
Aave's scaled balance against the liquidity index rather than running its own
share ratio.

## The invariant

```
total_supply × SCALED_UNIT  <=  the vault's XOXNO scaled position (Ray)
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

The full SEP-56 interface, exactly as the standard declares it:

```rust
total_supply() -> i128                         query_asset() -> Address
total_assets() -> i128
convert_to_shares(assets) -> i128              convert_to_assets(shares) -> i128
max_deposit(receiver) -> i128                  preview_deposit(assets) -> i128
deposit(assets, receiver, from, operator) -> shares
max_mint(receiver) -> i128                     preview_mint(shares) -> i128
mint(shares, receiver, from, operator) -> assets
max_withdraw(owner) -> i128                    preview_withdraw(assets) -> i128
withdraw(assets, receiver, owner, operator) -> shares
max_redeem(owner) -> i128                      preview_redeem(shares) -> i128
redeem(shares, receiver, owner, operator) -> assets
```

with the standard's `Deposit` and `Withdraw` events, and the full SEP-41 token
surface on the same address. YBC itself calls only `query_asset`,
`convert_to_assets`, `deposit` and `redeem`. There are no fees, so every
`preview_*` equals the matching conversion with the standard's rounding:
`deposit` and `redeem` round down, `mint` and `withdraw` round up, and every
rounding falls in the vault's favour (`src/tests/conformance.rs`).

Beyond the standard: `get_protocol() -> Address` (the XOXNO controller),
informational, read by the YBC indexer so a curator can confirm the protocol
behind a vault; nothing on chain calls it. `mint` and `withdraw` keep the
crate invariant for the same reason `deposit` and `redeem` do: rounding the
caller's side up at share precision always covers the controller's own
rounding at Ray precision (`vault.rs` proptests).

There is no privileged function and no admin. XOXNO has no on-chain rewards to
harvest (see "There is no harvest" below), and a token that lands on the vault
address by accident stays there: an admin rescue was judged not worth the
trust it requires.

## Deployment

```bash
stellar contract deploy --wasm target/wasm32v1-none/release/xoxno_vault.wasm \
  -- --controller <CONTROLLER> --asset <USDC_SAC> \
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
the CLI wrapper, and a bare cargo build fails in the SDK's build script.
`make build` adds the release workflow's flags.

## What is shared and what is XOXNO's

The share token (OpenZeppelin `Base`), the SEP-56 declaration
(`impl_sep56!` writes `preview_deposit`, `preview_redeem`, `max_redeem`,
`max_withdraw` and `max_mint`, and compile-checks the rest), the
operator-allowance rule on `redeem` and `withdraw`, the positive-amount guard,
the TTL policy, the `Deposit`/`Withdraw` events and the widening multiply come
from `vault-common`. `src/tests/conformance.rs` binds this crate's fixture
to `vault-testkit`, which runs the same 36 properties against every adapter.

This crate owns only what is XOXNO's: the `lending/` mirror of
`xoxno-contract-sdk` (see below), the account-id sentinel, the index maths
(`vault.rs`), the `max_deposit`, `withdraw_limit`, `total_assets`,
`account_id` and `config` views (`max_withdraw` and `max_redeem` are capped by
`withdraw_limit`: the pool's cash, and 0 while the market is paused or
frozen), and the four XOXNO-specific errors
(`ZeroAssetRedeem = 300`, `NoAccount = 301`, `UnsupportedDecimals = 302`,
`MintShortfall = 303`; shared codes are 10–49, OpenZeppelin's are 100–199).
The mock controller the tests run on lives in
`vault_testkit::protocols::xoxno`, shared with YBC's market tests.

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
see `src/tests/hazards.rs`.

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

## Reference release and the `xoxno-contract-sdk` migration

XOXNO publishes [`xoxno-contract-sdk`](https://crates.io/crates/xoxno-contract-sdk)
(0.1.0, 2026-09-24, MIT): generated clients, the protocol constants, and a
`LendingFixture` that deploys the real protocol into a test `Env`. It is built
on soroban-sdk 28 and its embedded WASM is protocol 28. This workspace is held
on soroban-sdk 26 by `stellar-tokens` 0.7.2, and a protocol-28 WASM does not
load on a 26 host (`Error(WasmVm, InvalidInput)`, "contract protocol number is
newer than host"), so neither linking the crate nor vendoring its binaries
works until the workspace moves to 28.

Until then `src/lending/` mirrors the slice of the SDK this vault uses, under
the SDK's own paths, names, constants and doc comments:

| this crate                                  | `xoxno-contract-sdk`                           |
|---------------------------------------------|------------------------------------------------|
| `crate::lending::constants::{RAY, NEW_ACCOUNT, WITHDRAW_ALL, …}` | `lending::constants::*`   |
| `crate::lending::controller::{HubAssetKey, MarketIndexRaw, …}`   | `lending::controller::*`  |
| `crate::lending::ControllerClient`          | `lending::ControllerClient`                    |
| `crate::lending::helpers::authorize_transfer_as_current` | `lending::helpers::…`           |
| `vault_testkit::protocols::xoxno::MockController` | `testutils::LendingFixture`              |

The migration is a path change (`crate::lending` → `xoxno_contract_sdk::lending`),
deleting `src/lending/`, and swapping the mock for `LendingFixture` in the
tests. The units this crate assumes are the SDK's documented ones: "Shares,
indexes and rates are RAY (1e27)", which is what `vault::SCALED_UNIT` converts
from.

Every struct field and the nine controller signatures used here were checked
with `stellar contract info interface` against both of these and match:

| binary | protocol | SHA-256 |
|---|---|---|
| SDK 0.1.0 `wasm/deploy/controller.wasm` (rs-lending-xlm `v1.1.0`, commit `1053ae0`, mainnet `CAUCMIN5…`) | 28 | `2dd536b06aab811801b5f3f2ad44629910b825c1535f85b420d16bbc4e6ce8b2` |
| SDK 0.1.0 `wasm/deploy/pool.wasm` (mainnet `CBXRNDQM…`) | 28 | `510246738d4533c4f30cb509a0497c1940856c1093d34a963e7ba7a47ff3c3bc` |
| testnet controller `CCXRWJ6S…` (the deployment this vault runs against) | 27 | `dd6dd50bc0bbf2834bfa2215bc66f6911aeba05c984a42590fe37df4da46e29d` |

The v1.1.0 controller adds `get_spoke_asset_flags_epoch` and
`relax_spoke_asset_flags` over the testnet build; nothing this vault calls
changed.

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
