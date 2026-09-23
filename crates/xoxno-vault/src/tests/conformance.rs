//! The workspace conformance suite, run against this adapter. The properties
//! themselves live in `vault-testkit`; this file only binds the fixture.

vault_testkit::conformance_tests!(super::fixture::VaultFixture::new());
