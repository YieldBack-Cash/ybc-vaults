# ybc-vaults

Every SEP-56 vault adapter a [YieldBack.Cash](https://github.com/YieldBack-Cash/ybc-contracts)
market can be created on, in one workspace. Adding a yield protocol is adding
one crate.

| Crate | Kind | What it is |
|---|---|---|
| `crates/vault-common` | rlib | The share token (OpenZeppelin `Base`), the SEP-56 interface declared once (`impl_sep56!` writes the five derived views and compile-checks the rest), operator-allowance rule, amount guard, widening `mul_div_floor`/`mul_div_ceil`, TTL policy, the SEP-56 `Deposit`/`Withdraw` events, shared error codes. **No rate math.** |
| `crates/vault-testkit` | rlib, test-only | The conformance suite every adapter runs: 36 properties, the SEP-56 rules (rounding, previews, limits, events) plus the ones drawn from the YBC threat model. Also the protocol fixtures (`protocols::blend`, a real Blend deployment; `protocols::xoxno`, the mock controller), which `ybc-contracts/tests/vaults` reuses. |
| `crates/xoxno-vault` | contract | XOXNO lending adapter. Shares mirror XOXNO's scaled unit; the rate is the market's supply index. |
| `crates/blend-vault` | contract | Blend pool adapter. Shares are a ratio over the vault's bToken position so harvested BLND emissions accrue to holders; the rate is that ratio through the pool's `b_rate`. |
| `wasm/blend/` | binaries | The Blend protocol, vendored from `blend-contract-sdk` 2.25.0 for the pool client and the test fixture. See its `MANIFEST.md`. |

The core protocol does not know which adapter backs a market. It calls only
`query_asset`, `convert_to_assets`, `deposit` and `redeem`, plus SEP-41 on the
same address. Everything else an adapter exposes is for operators and the
indexer.

## Build and test

```bash
make build        # stellar contract build --optimize; one .wasm per adapter
make test         # cargo test --workspace
```

`rust-toolchain.toml` pins one toolchain for every crate. The auditor rebuilds
and compares WASM hashes, and toolchain drift changes the hash.

## Adding an adapter

1. `crates/<protocol>-vault` with a hand-written client for the protocol
   (`xoxno-vault/src/lending/controller.rs` is the model: declare only the calls you
   make, and remember Soroban matches struct fields by *name*).
2. `#[contract] pub struct MyVault;` then `vault_common::impl_share_token!(MyVault);`
   for SEP-41 and `vault_common::impl_sep56!(MyVault);` for SEP-56. Set
   metadata with 7 decimals in the constructor.
3. The eleven SEP-56 functions that depend on the protocol (`query_asset`,
   `total_assets`, the two conversions, `max_deposit`, `preview_mint`,
   `preview_withdraw`, `deposit`, `mint`, `withdraw`, `redeem`), each taking
   `e: &Env`, plus `withdraw_limit(e) -> i128`: the most the protocol can pay
   out right now (0 while halted), which caps `max_withdraw` and `max_redeem`.
   `impl_sep56!` supplies the other five and refuses to compile until all are
   there with the standard's signatures. Call
   `vault_common::guard::require_positive` first,
   `vault_common::auth::spend_operator_allowance` on a delegated exit, and
   `Base::mint` / `Base::update` for the ledger. Own the rate math in your crate.
4. Protocol-specific errors in your own `#[contracterror]`, numbered in your
   100-block (blend 200s, xoxno 300s, next 400s). Never 100–199: that is
   OpenZeppelin's.
5. A fixture for the protocol under `vault-testkit/src/protocols/` (so YBC's
   market tests can reuse it), a fixture implementing
   `vault_testkit::ConformanceFixture`, and one line:
   `vault_testkit::conformance_tests!(Fixture::new());`. Test only what is the
   protocol's; the conformance suite already covers the standard.

Nothing in `ybc-contracts`, the indexer or the frontend changes: the indexer
learns the protocol behind a vault from `get_protocol() -> Address`, the one
informational view every adapter exposes beyond SEP-56.

## History

`crates/xoxno-vault` and `crates/blend-vault` were imported with `git subtree`
from their original repositories, so `git log --follow` and `git blame` reach
back through the move. `blend-vault` carries Script3's fee-vault-v2 history and
license; see `ybc-contracts/docs/VAULT_MONOREPO_PLAN.md` §3.6.
