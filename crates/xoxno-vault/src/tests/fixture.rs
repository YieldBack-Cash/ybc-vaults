use soroban_sdk::{
    testutils::Address as _,
    token::{StellarAssetClient, TokenClient},
    Address, Env, String,
};
use vault_testkit::ConformanceFixture;

use crate::contract::{XoxnoVault, XoxnoVaultClient};
use crate::lending::constants::RAY;
use crate::testutils::{MockController, MockControllerClient};

pub const HUB_ID: u32 = 1;
pub const SPOKE_ID: u32 = 1;

pub struct VaultFixture<'a> {
    pub e: Env,
    pub vault: XoxnoVaultClient<'a>,
    pub vault_address: Address,
    pub controller: MockControllerClient<'a>,
    pub controller_address: Address,
    pub token: TokenClient<'a>,
    pub asset: Address,
    pub user: Address,
    pub other: Address,
}

impl<'a> VaultFixture<'a> {
    pub fn new() -> Self {
        let e = Env::default();
        // `allowing_non_root_auth` is required, not incidental: the mock
        // controller moves a depositor's tokens from inside a contract frame,
        // which plain `mock_all_auths` refuses unless that address already
        // authorized at the root of the call tree.
        e.mock_all_auths_allowing_non_root_auth();
        e.cost_estimate().budget().reset_unlimited();

        let issuer = Address::generate(&e);
        let user = Address::generate(&e);
        let other = Address::generate(&e);

        let sac = e.register_stellar_asset_contract_v2(issuer.clone());
        let asset = sac.address();

        let controller_address = e.register(MockController, (asset.clone(), HUB_ID));
        let vault_address = e.register(
            XoxnoVault,
            (
                controller_address.clone(),
                asset.clone(),
                HUB_ID,
                SPOKE_ID,
                String::from_str(&e, "XOXNO USDC Vault"),
                String::from_str(&e, "xvUSDC"),
            ),
        );

        let fixture = VaultFixture {
            vault: XoxnoVaultClient::new(&e, &vault_address),
            controller: MockControllerClient::new(&e, &controller_address),
            token: TokenClient::new(&e, &asset),
            controller_address,
            vault_address,
            asset,
            user,
            other,
            e,
        };

        fixture.mint_to(&fixture.user.clone(), 1_000_000_0000000);
        fixture
    }

    pub fn mint_to(&self, to: &Address, amount: i128) {
        StellarAssetClient::new(&self.e, &self.asset).mint(to, &amount);
    }

    /// Interest accrual: raise the market's supply index by `bps`.
    ///
    /// The matching cash is minted to the pool as well. Raising the index alone
    /// would create a claim with nothing behind it, and the first holder to
    /// redeem their gain would fail on the pool's balance rather than on
    /// anything the vault did: a mock artefact masquerading as a vault bug.
    /// Real accrual is funded by borrowers paying interest.
    pub fn accrue(&self, bps: i128) {
        let before = self.vault.total_assets();
        let index = self.index();
        self.controller
            .set_supply_index(&(index + index * bps / 10_000));
        let after = self.vault.total_assets();
        if after > before {
            self.mint_to(&self.controller_address.clone(), after - before);
        }
    }

    /// A `seize_positions`-style write-down: drop the index by `bps`.
    pub fn write_down(&self, bps: i128) {
        let index = self
            .controller
            .get_market_index(&self.hub_asset())
            .supply_index;
        self.controller
            .set_supply_index(&(index - index * bps / 10_000));
    }

    pub fn hub_asset(&self) -> crate::lending::controller::HubAssetKey {
        crate::lending::controller::HubAssetKey {
            asset: self.asset.clone(),
            hub_id: HUB_ID,
        }
    }

    pub fn index(&self) -> i128 {
        self.controller
            .get_market_index(&self.hub_asset())
            .supply_index
    }

    pub fn assert_index_starts_at_ray(&self) {
        assert_eq!(self.index(), RAY);
    }
}

/// What the shared conformance suite needs. See `tests/conformance.rs`.
impl ConformanceFixture for VaultFixture<'_> {
    fn env(&self) -> &Env {
        &self.e
    }

    fn vault(&self) -> Address {
        self.vault_address.clone()
    }

    fn asset(&self) -> Address {
        self.asset.clone()
    }

    fn mint(&self, to: &Address, amount: i128) {
        self.mint_to(to, amount);
    }

    fn accrue(&self, bps: i128) {
        VaultFixture::accrue(self, bps);
    }

    fn write_down(&self, bps: i128) -> bool {
        VaultFixture::write_down(self, bps);
        true
    }
}
