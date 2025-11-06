use {
    super::ProviderConfig,
    crate::providers::{Priority, Weight},
    std::collections::HashMap,
    tracing::error,
};

#[derive(Debug)]
pub struct AllnodesConfig {
    pub supported_chains: HashMap<String, (String, Weight)>,
    pub supported_ws_chains: HashMap<String, (String, Weight)>,
    pub chain_subdomains: HashMap<String, String>,
}

impl AllnodesConfig {
    pub fn new(api_tokens_json: String) -> Self {
        let (supported_chains, chain_subdomains) =
            extract_supported_chains_and_subdomains(api_tokens_json.clone());
        let supported_ws_chains = extract_ws_supported_chains_and_subdomains(api_tokens_json);
        Self {
            supported_chains,
            supported_ws_chains,
            chain_subdomains,
        }
    }
}

impl ProviderConfig for AllnodesConfig {
    fn supported_chains(self) -> HashMap<String, (String, Weight)> {
        self.supported_chains
    }

    fn supported_ws_chains(self) -> HashMap<String, (String, Weight)> {
        self.supported_ws_chains
    }

    fn provider_kind(&self) -> crate::providers::ProviderKind {
        crate::providers::ProviderKind::Allnodes
    }
}

fn extract_supported_chains_and_subdomains(
    access_tokens_json: String,
) -> (HashMap<String, (String, Weight)>, HashMap<String, String>) {
    let access_tokens: HashMap<String, String> = match serde_json::from_str(&access_tokens_json) {
        Ok(tokens) => tokens,
        Err(_) => {
            error!(
                "Failed to parse JSON with API access tokens for Allnodes provider. Using empty \
                 tokens."
            );
            return (HashMap::new(), HashMap::new());
        }
    };

    // Keep in-sync with SUPPORTED_CHAINS.md
    // Supported chains list format: chain ID, subdomain, priority
    let supported_chain_ids = HashMap::from([
        ("eip155:1", ("eth57873", Priority::Max)),
        ("eip155:8453", ("base57873", Priority::Max)),
        ("eip155:56", ("bnb57873", Priority::Max)),
        ("eip155:137", ("pol57873", Priority::Max)),
    ]);

    let access_tokens_with_weights: HashMap<String, (String, Weight)> = supported_chain_ids
        .iter()
        .filter_map(|(&key, (_, weight))| {
            if let Some(token) = access_tokens.get(key) {
                match Weight::new(*weight) {
                    Ok(weight) => Some((key.to_string(), (token.to_string(), weight))),
                    Err(_) => {
                        error!(
                            "Failed to create Weight for key {} in Allnodes provider",
                            key
                        );
                        None
                    }
                }
            } else {
                error!(
                    "Allnodes provider API access token for {} is not present, skipping it",
                    key
                );
                None
            }
        })
        .collect();
    let chain_ids_subdomains: HashMap<String, String> = supported_chain_ids
        .iter()
        .map(|(&key, (subdomain, _))| (key.to_string(), subdomain.to_string()))
        .collect();

    (access_tokens_with_weights, chain_ids_subdomains)
}

fn extract_ws_supported_chains_and_subdomains(
    access_tokens_json: String,
) -> HashMap<String, (String, Weight)> {
    let access_tokens: HashMap<String, String> = match serde_json::from_str(&access_tokens_json) {
        Ok(tokens) => tokens,
        Err(_) => {
            error!(
                "Failed to parse JSON with API ws access tokens for Allnodes provider. Using empty \
                 tokens."
            );
            return HashMap::new();
        }
    };

    // Keep in-sync with SUPPORTED_CHAINS.md
    // Supported chains list format: chain ID, subdomain, priority
    let supported_chain_ids = HashMap::from([
        ("eip155:1", ("eth57873", Priority::Max)),
        ("eip155:8453", ("base57873", Priority::Max)),
        ("eip155:56", ("bnb57873", Priority::Max)),
        ("eip155:137", ("pol57873", Priority::Max)),
    ]);

    let access_tokens_with_weights: HashMap<String, (String, Weight)> = supported_chain_ids
        .iter()
        .filter_map(|(&key, (_, weight))| {
            if let Some(token) = access_tokens.get(key) {
                match Weight::new(*weight) {
                    Ok(weight) => Some((key.to_string(), (token.to_string(), weight))),
                    Err(_) => {
                        error!(
                            "Failed to create Weight for key {} in Allnodes provider",
                            key
                        );
                        None
                    }
                }
            } else {
                error!(
                    "Allnodes provider API ws access token for {} is not present, skipping it",
                    key
                );
                None
            }
        })
        .collect();

    access_tokens_with_weights
}
