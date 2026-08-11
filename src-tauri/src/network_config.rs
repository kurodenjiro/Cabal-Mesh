//! Which chain to talk to, and where its contracts live.
//!
//! # Why environment variables had to go
//!
//! Contract addresses came from bare `std::env::var` with no fallback. On
//! desktop that works because a `.env` file is loaded at startup. On mobile
//! there is **no environment to read and no file to load**, so every address
//! resolved to `None` and every contract call failed — with an error that
//! looked like a chain problem rather than a configuration one.
//!
//! Addresses are now a compiled-in table keyed by network, overridable at
//! runtime for anyone pointing the app at their own deployment.
//!
//! # Why the default is a testnet
//!
//! Fuji, not mainnet. This build is still moving, the escrow and marketplace
//! contracts are unaudited, and a wrong default here spends real money rather
//! than displaying something wrong. Promoting to mainnet is one config change
//! and should be a deliberate one.

use serde::{Deserialize, Serialize};

/// A chain this app knows how to talk to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Network {
    /// Avalanche Fuji testnet. The default, deliberately.
    #[default]
    Fuji,
    /// Avalanche mainnet. Real funds.
    Mainnet,
}

impl Network {
    /// Human-readable name for logs and the profile screen.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Fuji => "Avalanche Fuji",
            Self::Mainnet => "Avalanche",
        }
    }

    /// Whether transactions here move real value.
    ///
    /// The UI uses this to mark testnet plainly, so nobody mistakes a test
    /// balance for a real one.
    #[must_use]
    pub const fn is_testnet(self) -> bool {
        matches!(self, Self::Fuji)
    }

    /// Default JSON-RPC endpoint.
    #[must_use]
    pub const fn default_rpc_url(self) -> &'static str {
        match self {
            Self::Fuji => "https://api.avax-test.network/ext/bc/C/rpc",
            Self::Mainnet => "https://api.avax.network/ext/bc/C/rpc",
        }
    }

    /// Contract addresses for this network.
    ///
    /// Empty where nothing is deployed yet. An absent address surfaces as a
    /// clear "not configured" error at the first call rather than a
    /// plausible-looking wrong address, which is why there is no placeholder.
    ///
    /// The Fuji voucher and marketplace addresses are the second deployment,
    /// not the one in the repo's early history. The first pair had an open
    /// mint on the voucher and a buyer-only escrow on the marketplace, and
    /// both are fixed in the contract source rather than worked around here.
    /// The old addresses are deliberately not kept as a fallback: a token from
    /// the open-mint contract is not an authentic module, and silently
    /// accepting one would defeat the point of redeploying.
    /// See `contracts/deployments/fuji.json` for the current record.
    #[must_use]
    pub const fn contracts(self) -> Contracts {
        match self {
            Self::Fuji => Contracts {
                escrow: Some("0xCaFF53657191d75Aa4f5C2182210302656d8B392"),
                marketplace: Some("0xb6F2B9415fc599130084b7F20B84738aCBB15930"),
                voucher: Some("0x3649E46eCD6A0bd187f0046C4C35a7B31C92bA1E"),
                // No reviewed deployment yet. Never substitute the legacy
                // voucher: it has no authentic structured module semantics.
                modules: None,
            },
            Self::Mainnet => Contracts {
                escrow: None,
                marketplace: None,
                voucher: None,
                modules: None,
            },
        }
    }
}

/// Deployed contract addresses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Contracts {
    pub escrow: Option<&'static str>,
    pub marketplace: Option<&'static str>,
    pub voucher: Option<&'static str>,
    pub modules: Option<&'static str>,
}

/// Resolved chain configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkConfig {
    #[serde(default)]
    pub network: Network,
    /// Overrides the network's default endpoint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rpc_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub escrow_address: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub marketplace_address: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voucher_address: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modules_address: Option<String>,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            network: Network::default(),
            rpc_url: None,
            escrow_address: None,
            marketplace_address: None,
            voucher_address: None,
            modules_address: None,
        }
    }
}

impl NetworkConfig {
    /// Loads configuration, layering: compiled-in defaults, then the config
    /// file, then environment variables on desktop.
    ///
    /// The environment layer is desktop-only and exists so the local two-node
    /// test and contract deployments keep working. Mobile has no environment,
    /// which is the whole reason this type exists.
    #[must_use]
    pub fn load(store: &cabal_store::JsonStore) -> Self {
        let mut config: Self = store.load_or(Self::default());

        #[cfg(desktop)]
        config.apply_environment_overrides();

        config
    }

    #[cfg(desktop)]
    fn apply_environment_overrides(&mut self) {
        fn var(name: &str) -> Option<String> {
            std::env::var(name).ok().filter(|value| !value.is_empty())
        }

        if let Some(url) = var("AVAX_RPC_URL") {
            self.rpc_url = Some(url);
        }
        if let Some(address) = var("ESCROW_CONTRACT_ADDRESS") {
            self.escrow_address = Some(address);
        }
        if let Some(address) = var("MARKETPLACE_CONTRACT_ADDRESS") {
            self.marketplace_address = Some(address);
        }
        if let Some(address) = var("VOUCHER_CONTRACT_ADDRESS") {
            self.voucher_address = Some(address);
        }
        if let Some(address) = var("MODULES_CONTRACT_ADDRESS") {
            self.modules_address = Some(address);
        }
    }

    /// The endpoint to use.
    #[must_use]
    pub fn rpc_url(&self) -> String {
        self.rpc_url
            .clone()
            .unwrap_or_else(|| self.network.default_rpc_url().to_owned())
    }

    /// Escrow address: explicit override, else the network's compiled-in value.
    #[must_use]
    pub fn escrow(&self) -> Option<String> {
        self.escrow_address
            .clone()
            .or_else(|| self.network.contracts().escrow.map(ToOwned::to_owned))
    }

    /// Marketplace address, resolved as [`NetworkConfig::escrow`].
    #[must_use]
    pub fn marketplace(&self) -> Option<String> {
        self.marketplace_address
            .clone()
            .or_else(|| self.network.contracts().marketplace.map(ToOwned::to_owned))
    }

    /// Voucher address, resolved as [`NetworkConfig::escrow`].
    #[must_use]
    pub fn voucher(&self) -> Option<String> {
        self.voucher_address
            .clone()
            .or_else(|| self.network.contracts().voucher.map(ToOwned::to_owned))
    }

    /// Authentic module address, resolved as [`NetworkConfig::escrow`].
    ///
    /// Absence is intentional until a reviewed deployment is published. There
    /// is no fallback to the legacy voucher collection.
    #[must_use]
    pub fn modules(&self) -> Option<String> {
        self.modules_address
            .clone()
            .or_else(|| self.network.contracts().modules.map(ToOwned::to_owned))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn the_default_is_a_testnet() {
        // A wrong default here spends real money rather than showing something
        // wrong, so mainnet must never be the fallback.
        assert_eq!(Network::default(), Network::Fuji);
        assert!(Network::default().is_testnet());
    }

    #[test]
    fn each_network_has_its_own_endpoint() {
        assert!(Network::Fuji.default_rpc_url().contains("avax-test"));
        assert!(!Network::Mainnet.default_rpc_url().contains("avax-test"));
    }

    #[test]
    fn absent_config_yields_the_testnet_endpoint() {
        let dir = TempDir::new().unwrap();
        let store = cabal_store::JsonStore::new(dir.path().join("network.json"));
        assert!(NetworkConfig::load(&store).rpc_url().contains("avax-test"));
    }

    #[test]
    fn an_explicit_address_wins_over_the_compiled_table() {
        let config = NetworkConfig {
            escrow_address: Some("0x1234".into()),
            ..NetworkConfig::default()
        };
        assert_eq!(config.escrow().as_deref(), Some("0x1234"));
    }

    #[test]
    fn an_undeployed_contract_is_none_rather_than_a_placeholder() {
        // A plausible-looking wrong address fails as a chain error. None fails
        // as "not configured", which is the truth and is actionable. Nothing
        // is deployed to mainnet, so nothing is claimed for it.
        let mainnet = Network::Mainnet.contracts();
        assert!(mainnet.escrow.is_none());
        assert!(mainnet.marketplace.is_none());
        assert!(mainnet.voucher.is_none());
        assert!(mainnet.modules.is_none());
    }

    #[test]
    fn the_testnet_deployment_is_compiled_in() {
        // The addresses live in code rather than in an env var because mobile
        // has no environment to read one from — that is the whole reason this
        // table exists, so an empty table for the default network would put
        // every phone back where it started.
        let fuji = Network::Fuji.contracts();
        for address in [fuji.escrow, fuji.marketplace, fuji.voucher] {
            let address = address.expect("Fuji contracts are deployed");
            assert!(address.starts_with("0x") && address.len() == 42, "{address} is not an address");
        }
        assert!(fuji.modules.is_none(), "unreviewed legacy voucher must not become a module collection");
    }

    #[test]
    fn a_partial_config_file_still_loads() {
        // Config gains fields over time; an older file must not become
        // unloadable when it does.
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("network.json");
        std::fs::write(&path, r#"{"network":"mainnet"}"#).unwrap();

        let config = NetworkConfig::load(&cabal_store::JsonStore::new(&path));
        assert_eq!(config.network, Network::Mainnet);
        assert!(!config.network.is_testnet());
    }
}
