# Vendored Blend protocol binaries

Test fixtures and client generation only. None of these is shipped in a YBC
artefact; `blend-vault` `contractimport!`s `pool.wasm` to generate its typed
pool client, and the test harness deploys all of them to stand up a real Blend
deployment in a Soroban test `Env`.

Source: the `blend-contract-sdk` crate, version **2.25.0**, directory `wasm/`.
Copied unchanged. `SHA256SUMS` beside this file is the checksum manifest;
verify with `sha256sum -c SHA256SUMS`.

| File | Built with |
|---|---|
| `pool.wasm`, `pool_factory.wasm`, `backstop.wasm` | soroban-sdk 22.0.7, rustc 1.81.0 |
| `emitter.wasm`, `comet.wasm`, `comet_factory.wasm` | soroban-sdk 20.5.0, rustc 1.77.2 |

Why vendored rather than depended on: `blend-contract-sdk` pins its own
`soroban-sdk` (25.0.1 at 2.25.0) and cannot be linked into a workspace on
soroban-sdk 26. The binaries themselves are SDK-agnostic; an older-protocol
contract runs on a newer test host. When Blend publishes a release on SDK 26,
the crate dependency can replace this directory again.
