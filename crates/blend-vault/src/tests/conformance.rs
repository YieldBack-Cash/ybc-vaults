//! The workspace conformance suite, run against this adapter on a real Blend
//! pool. The properties themselves live in `vault-testkit`; this file only
//! binds the fixture.

vault_testkit::conformance_tests!(crate::testutils::BlendConformanceFixture::new());
