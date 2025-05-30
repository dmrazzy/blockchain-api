use {
    super::ProviderConfig,
    crate::providers::{Priority, Weight},
    std::collections::HashMap,
};

#[derive(Debug)]
pub struct InfuraConfig {
    pub project_id: String,

    pub supported_chains: HashMap<String, (String, Weight)>,

    pub supported_ws_chains: HashMap<String, (String, Weight)>,
}

impl InfuraConfig {
    pub fn new(project_id: String) -> Self {
        Self {
            project_id,
            supported_chains: default_supported_chains(),
            supported_ws_chains: default_ws_supported_chains(),
        }
    }
}

impl ProviderConfig for InfuraConfig {
    fn supported_chains(self) -> HashMap<String, (String, Weight)> {
        self.supported_chains
    }

    fn supported_ws_chains(self) -> HashMap<String, (String, Weight)> {
        self.supported_ws_chains
    }

    fn provider_kind(&self) -> crate::providers::ProviderKind {
        crate::providers::ProviderKind::Infura
    }
}

fn default_supported_chains() -> HashMap<String, (String, Weight)> {
    // Keep in-sync with SUPPORTED_CHAINS.md

    HashMap::from([
        // Ethereum
        (
            "eip155:1".into(),
            ("mainnet".into(), Weight::new(Priority::Minimal).unwrap()),
        ),
        (
            "eip155:11155111".into(),
            ("sepolia".into(), Weight::new(Priority::Minimal).unwrap()),
        ),
        // Optimism
        (
            "eip155:10".into(),
            (
                "optimism-mainnet".into(),
                Weight::new(Priority::Custom(1)).unwrap(),
            ),
        ),
        (
            "eip155:11155420".into(),
            (
                "optimism-sepolia".into(),
                Weight::new(Priority::Custom(1)).unwrap(),
            ),
        ),
        // Arbitrum
        (
            "eip155:42161".into(),
            (
                "arbitrum-mainnet".into(),
                Weight::new(Priority::Custom(1)).unwrap(),
            ),
        ),
        (
            "eip155:421614".into(),
            (
                "arbitrum-sepolia".into(),
                Weight::new(Priority::Custom(1)).unwrap(),
            ),
        ),
        // Polygon
        (
            "eip155:137".into(),
            (
                "polygon-mainnet".into(),
                Weight::new(Priority::Custom(1)).unwrap(),
            ),
        ),
        // Polygon Amoy
        (
            "eip155:80002".into(),
            (
                "polygon-amoy".into(),
                Weight::new(Priority::Custom(1)).unwrap(),
            ),
        ),
        // Celo
        (
            "eip155:42220".into(),
            (
                "celo-mainnet".into(),
                Weight::new(Priority::Custom(1)).unwrap(),
            ),
        ),
        // Linea Mainnet
        (
            "eip155:59144".into(),
            (
                "linea-mainnet".into(),
                Weight::new(Priority::Custom(1)).unwrap(),
            ),
        ),
        // Mantle
        (
            "eip155:5000".into(),
            (
                "mantle-mainnet".into(),
                Weight::new(Priority::Custom(1)).unwrap(),
            ),
        ),
        // Base Mainnet
        (
            "eip155:8453".into(),
            (
                "base-mainnet".into(),
                Weight::new(Priority::Custom(1)).unwrap(),
            ),
        ),
        // Base Sepolia
        (
            "eip155:84532".into(),
            (
                "base-sepolia".into(),
                Weight::new(Priority::Custom(1)).unwrap(),
            ),
        ),
        // BSC Mainnet
        (
            "eip155:56".into(),
            (
                "bsc-mainnet".into(),
                Weight::new(Priority::Custom(1)).unwrap(),
            ),
        ),
        // BSC Testnet
        (
            "eip155:97".into(),
            (
                "bsc-testnet".into(),
                Weight::new(Priority::Custom(1)).unwrap(),
            ),
        ),
        // ZkSync Mainnet
        (
            "eip155:324".into(),
            (
                "zksync-mainnet".into(),
                Weight::new(Priority::Custom(1)).unwrap(),
            ),
        ),
        // Unichain Sepolia
        (
            "eip155:1301".into(),
            (
                "unichain-sepolia".into(),
                Weight::new(Priority::Custom(1)).unwrap(),
            ),
        ),
    ])
}

fn default_ws_supported_chains() -> HashMap<String, (String, Weight)> {
    // Keep in-sync with SUPPORTED_CHAINS.md

    HashMap::from([
        // Ethereum
        (
            "eip155:1".into(),
            ("mainnet".into(), Weight::new(Priority::Normal).unwrap()),
        ),
        (
            "eip155:11155111".into(),
            ("sepolia".into(), Weight::new(Priority::Normal).unwrap()),
        ),
        // Optimism
        (
            "eip155:10".into(),
            (
                "optimism-mainnet".into(),
                Weight::new(Priority::Normal).unwrap(),
            ),
        ),
        (
            "eip155:11155420".into(),
            (
                "optimism-sepolia".into(),
                Weight::new(Priority::Normal).unwrap(),
            ),
        ),
        // Arbitrum
        (
            "eip155:42161".into(),
            (
                "arbitrum-mainnet".into(),
                Weight::new(Priority::Normal).unwrap(),
            ),
        ),
        (
            "eip155:421614".into(),
            (
                "arbitrum-sepolia".into(),
                Weight::new(Priority::Normal).unwrap(),
            ),
        ),
    ])
}
