use {
    super::{
        BalanceProvider, FungiblePriceProvider, HistoryProvider, PriceResponseBody,
        SupportedCurrencies,
    },
    crate::{
        env::SolScanConfig,
        error::{RpcError, RpcResult},
        handlers::{
            balance::{
                BalanceItem, BalanceQuantity, BalanceQueryParams, BalanceResponseBody,
                TokenMetadataCacheItem,
            },
            fungible_price::FungiblePriceItem,
            history::{
                HistoryQueryParams, HistoryResponseBody, HistoryTransaction,
                HistoryTransactionFungibleInfo, HistoryTransactionMetadata,
                HistoryTransactionTransfer, HistoryTransactionTransferQuantity,
                HistoryTransactionURLItem,
            },
        },
        providers::{BalanceProviderFactory, ProviderKind, TokenMetadataCacheProvider},
        storage::error::StorageError,
        utils::crypto::{CaipNamespaces, SOLANA_NATIVE_TOKEN_ADDRESS},
        Metrics,
    },
    async_trait::async_trait,
    deadpool_redis::{redis::AsyncCommands, Pool},
    serde::{Deserialize, Serialize},
    std::{fmt, sync::Arc, time::SystemTime},
    tracing::log::error,
    url::Url,
};

const SOLANA_SOL_TOKEN_ICON: &str =
    "https://cdn.jsdelivr.net/gh/trustwallet/assets@master/blockchains/solana/info/logo.png";
const SOLANA_MAINNET_CHAIN_ID: &str = "solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp";
const ACCOUNT_TOKENS_URL: &str = "https://pro-api.solscan.io/v2.0/account/token-accounts";
const ACCOUNT_HISTORY_URL: &str = "https://pro-api.solscan.io/v2.0/account/transfer";
const TOKEN_METADATA_URL: &str = "https://pro-api.solscan.io/v2.0/token/meta";
const TOKEN_PRICE_URL: &str = "https://pro-api.solscan.io/v2.0/token/price";
const ACCOUNT_DETAIL_URL: &str = "https://pro-api.solscan.io/v2.0/account/detail";

// Caching TTL paramters
const PRICING_CACHE_TTL: u64 = 60 * 5; // 5 minutes

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
struct AccountDetailResponse {
    pub data: AccountDetail,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
struct AccountDetail {
    pub lamports: Option<usize>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
struct TokenInfoResponse {
    pub data: TokenMetaData,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
struct TokenMetaData {
    pub name: Option<String>,
    pub symbol: String,
    pub decimals: u8,
    pub icon: Option<String>,
    pub price: f64,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
struct TokenPriceResponse {
    pub data: Vec<TokenPriceResponseData>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
struct TokenPriceResponseData {
    pub price: f64,
}

pub struct SolScanProvider {
    provider_kind: ProviderKind,
    api_v2_token: String,
    http_client: reqwest::Client,
    redis_caching_pool: Option<Arc<Pool>>,
}

impl SolScanProvider {
    pub fn new(api_v2_token: String, redis_caching_pool: Option<Arc<Pool>>) -> Self {
        Self {
            provider_kind: ProviderKind::SolScan,
            api_v2_token,
            http_client: reqwest::Client::new(),
            redis_caching_pool,
        }
    }

    async fn send_request_v2(&self, url: Url) -> Result<reqwest::Response, reqwest::Error> {
        self.http_client
            .get(url)
            .header("token", self.api_v2_token.clone())
            .send()
            .await
    }

    /// Construct the cache key for the pricing
    fn format_cache_pricing_key(&self, address: &str) -> String {
        format!("solscan/pricing/{address}")
    }

    #[allow(dependency_on_unit_never_type_fallback)]
    async fn set_cache(
        &self,
        key: &str,
        value: &str,
        ttl: u64,
        metrics: Arc<Metrics>,
    ) -> Result<(), StorageError> {
        if let Some(redis_pool) = &self.redis_caching_pool {
            let mut cache = redis_pool.get().await.map_err(|e| {
                StorageError::Connection(format!("Error when getting the Redis pool instance {e}"))
            })?;
            let start = SystemTime::now();
            cache
                .set_ex(key, value, ttl)
                .await
                .map_err(|e| StorageError::Connection(format!("Error when seting cache: {e}")))?;
            metrics.add_non_rpc_providers_cache_latency(start);
        }
        Ok(())
    }

    #[allow(dependency_on_unit_never_type_fallback)]
    async fn get_cache(
        &self,
        key: &str,
        metrics: Arc<Metrics>,
    ) -> Result<Option<String>, StorageError> {
        if let Some(redis_pool) = &self.redis_caching_pool {
            let mut cache = redis_pool.get().await.map_err(|e| {
                StorageError::Connection(format!("Error when getting the Redis pool instance {e}"))
            })?;
            let start = SystemTime::now();
            let value = cache
                .get(key)
                .await
                .map_err(|e| StorageError::Connection(format!("Error when getting cache: {e}")))?;
            metrics.add_non_rpc_providers_cache_latency(start);
            return Ok(value);
        }
        Ok(None)
    }

    async fn token_price_request(
        &self,
        address: &str,
        metrics: Arc<Metrics>,
    ) -> Result<f64, RpcError> {
        // Check the price from the cache first
        if let Some(redis_pool) = self
            .get_cache(&self.format_cache_pricing_key(address), metrics.clone())
            .await?
        {
            return Ok(redis_pool.parse().unwrap_or_default());
        }

        let mut url =
            Url::parse(TOKEN_PRICE_URL).map_err(|_| RpcError::FungiblePriceParseURLError)?;
        url.query_pairs_mut().append_pair("address", address);

        let latency_start = SystemTime::now();
        let response = self.send_request_v2(url).await?;
        metrics.add_latency_and_status_code_for_provider(
            &self.provider_kind,
            response.status().into(),
            latency_start,
            None,
            Some(TOKEN_PRICE_URL.to_string()),
        );

        if !response.status().is_success() {
            error!(
                "Error on SolScan token price response. Status is not OK: {:?}",
                response.status(),
            );
            return Err(RpcError::FungiblePriceProviderError(
                "Token price provider response status is not success".to_string(),
            ));
        }
        let body = response.json::<TokenPriceResponse>().await?;
        let price = body
            .data
            .first()
            .ok_or_else(|| {
                RpcError::FungiblePriceProviderError(
                    "Empty price response from the provider".to_string(),
                )
            })?
            .price;

        // Cache the price from the response
        self.set_cache(
            &self.format_cache_pricing_key(address),
            &price.to_string(),
            PRICING_CACHE_TTL,
            metrics,
        )
        .await?;

        Ok(price)
    }

    async fn token_metadata_request(
        &self,
        address: &str,
        metrics: Arc<Metrics>,
    ) -> Result<TokenMetaData, RpcError> {
        let mut url =
            Url::parse(TOKEN_METADATA_URL).map_err(|_| RpcError::FungiblePriceParseURLError)?;
        url.query_pairs_mut().append_pair("address", address);

        let latency_start = SystemTime::now();
        let response = self.send_request_v2(url).await?;
        metrics.add_latency_and_status_code_for_provider(
            &self.provider_kind,
            response.status().into(),
            latency_start,
            None,
            Some(TOKEN_METADATA_URL.to_string()),
        );

        if !response.status().is_success() {
            error!(
                "Error on SolScan token metadata response. Status is not OK: {:?}",
                response.status(),
            );
            return Err(RpcError::FungiblePriceProviderError(
                "Token metadata provider response status is not success".to_string(),
            ));
        }
        let body = response.json::<TokenInfoResponse>().await?;
        let response = TokenMetaData {
            name: body.data.name,
            symbol: body.data.symbol,
            decimals: body.data.decimals,
            icon: body.data.icon,
            price: body.data.price,
        };
        Ok(response)
    }

    async fn get_token_info(
        &self,
        address: &str,
        metadata_cache: &Arc<dyn TokenMetadataCacheProvider>,
        metrics: Arc<Metrics>,
    ) -> Result<TokenMetaData, RpcError> {
        let price = self
            .token_price_request(SOLANA_NATIVE_TOKEN_ADDRESS, metrics.clone())
            .await?;
        // Respond instantly for the native token (SOL) metadata with making just a price request
        // since metadata is static
        if address == SOLANA_NATIVE_TOKEN_ADDRESS {
            return Ok(TokenMetaData {
                name: Some("Solana".to_string()),
                symbol: "SOL".to_string(),
                decimals: 9,
                icon: Some(SOLANA_SOL_TOKEN_ICON.to_string()),
                price,
            });
        }

        let caip10_address = format!("{SOLANA_MAINNET_CHAIN_ID}:{address}");
        match metadata_cache.get_metadata(&caip10_address).await {
            Ok(Some(metadata)) => {
                return Ok(TokenMetaData {
                    name: Some(metadata.name),
                    symbol: metadata.symbol,
                    decimals: metadata.decimals,
                    icon: Some(metadata.icon_url),
                    price,
                });
            }
            Ok(None) => {}
            Err(_) => {
                error!("Error when getting the token metadata from the cache");
            }
        }

        let metadata = self.token_metadata_request(address, metrics).await?;

        // Cache the metadata
        let token_metadata = TokenMetadataCacheItem {
            name: metadata.name.clone().unwrap_or(metadata.symbol.clone()),
            symbol: metadata.symbol.clone(),
            decimals: metadata.decimals,
            icon_url: metadata.icon.clone().unwrap_or_default(),
        };
        {
            let metadata_cache = metadata_cache.clone();
            let caip10_address = caip10_address.clone();
            tokio::spawn(async move {
                if let Err(e) = metadata_cache
                    .set_metadata(&caip10_address, &token_metadata)
                    .await
                {
                    error!("Error when setting the token metadata to the cache: {e}");
                }
            });
        }

        Ok(metadata)
    }

    // Get SOL address balance by getting account detail
    async fn get_sol_balance(&self, address: &str, metrics: Arc<Metrics>) -> Result<f64, RpcError> {
        let mut url = Url::parse(ACCOUNT_DETAIL_URL).map_err(|_| RpcError::BalanceParseURLError)?;
        url.query_pairs_mut().append_pair("address", address);

        let latency_start = SystemTime::now();
        let response = self.send_request_v2(url).await?;
        metrics.add_latency_and_status_code_for_provider(
            &self.provider_kind,
            response.status().into(),
            latency_start,
            None,
            Some(ACCOUNT_DETAIL_URL.to_string()),
        );

        if !response.status().is_success() {
            error!(
                "Error on SolScan account detail response. Status is not OK: {:?}",
                response.status(),
            );
            return Err(RpcError::BalanceProviderError);
        }
        let detail = response.json::<AccountDetailResponse>().await?;

        let lamports = detail.data.lamports.unwrap_or_default();
        let balance = lamports as f64 / 10f64.powf(9.0);

        Ok(balance)
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
struct TokensResponse {
    pub data: Vec<TokensResponseItem>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
struct TokensResponseItem {
    pub token_address: String,
    pub token_decimals: u8,
    pub amount: u64,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
struct HistoryResponse {
    pub success: bool,
    pub data: Vec<HistoryResponseItem>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
struct HistoryResponseItem {
    pub block_time: usize,
    pub block_id: usize,
    pub trans_id: String,
    pub activity_type: HistoryActivityType,
    pub from_address: String,
    pub to_address: String,
    pub token_address: String,
    pub token_decimals: u8,
    pub amount: usize,
    pub flow: HistoryDirectionType,
    pub time: String,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
#[serde(rename_all = "lowercase")]
enum HistoryDirectionType {
    In,
    Out,
}
impl fmt::Display for HistoryDirectionType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HistoryDirectionType::In => write!(f, "in"),
            HistoryDirectionType::Out => write!(f, "out"),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
enum HistoryActivityType {
    #[serde(rename = "ACTIVITY_SPL_TRANSFER")]
    Transfer,
    #[serde(rename = "ACTIVITY_SPL_BURN")]
    Burn,
    #[serde(rename = "ACTIVITY_SPL_MINT")]
    Mint,
    #[serde(rename = "ACTIVITY_SPL_CREATE_ACCOUNT")]
    CreateAccount,
    #[serde(rename = "ACTIVITY_SPL_CLOSE_ACCOUNT")]
    CloseAccount,
}

#[async_trait]
impl BalanceProvider for SolScanProvider {
    async fn get_balance(
        &self,
        address: String,
        _params: BalanceQueryParams,
        metadata_cache: &Arc<dyn TokenMetadataCacheProvider>,
        metrics: Arc<Metrics>,
    ) -> RpcResult<BalanceResponseBody> {
        let mut url = Url::parse(ACCOUNT_TOKENS_URL).map_err(|_| RpcError::BalanceParseURLError)?;
        url.query_pairs_mut().append_pair("address", &address);
        url.query_pairs_mut().append_pair("type", "token");
        url.query_pairs_mut().append_pair("hide_zero", "true");

        let latency_start = SystemTime::now();
        let response = self.send_request_v2(url).await?;
        metrics.add_latency_and_status_code_for_provider(
            &self.provider_kind,
            response.status().into(),
            latency_start,
            None,
            Some(ACCOUNT_TOKENS_URL.to_string()),
        );

        if !response.status().is_success() {
            error!(
                "Error on SolScan balance response. Status is not OK: {:?}",
                response.status(),
            );
            return Err(RpcError::BalanceProviderError);
        }
        let mut balances_vec: Vec<BalanceItem> = Vec::new();
        let body = response.json::<TokensResponse>().await?;
        for item in body.data {
            let token_price = &self
                .token_price_request(&item.token_address, metrics.clone())
                .await
                .unwrap_or(0.0);
            let token_metadata = self
                .get_token_info(&item.token_address, metadata_cache, metrics.clone())
                .await?;
            let decimal_amount = item.amount as f64 / 10f64.powf(item.token_decimals as f64);
            let balance_item = BalanceItem {
                name: token_metadata.name.unwrap_or(token_metadata.symbol.clone()),
                symbol: token_metadata.symbol,
                chain_id: Some(SOLANA_MAINNET_CHAIN_ID.to_string()),
                address: Some(item.token_address),
                value: Some(decimal_amount * token_price),
                price: *token_price,
                quantity: BalanceQuantity {
                    decimals: item.token_decimals.to_string(),
                    numeric: decimal_amount.to_string(),
                },
                icon_url: token_metadata.icon.unwrap_or_default(),
            };
            balances_vec.push(balance_item);
        }

        // Inject Solana native token (SOL) balance if not zero
        let sol_balance = self.get_sol_balance(&address, metrics.clone()).await?;
        if sol_balance > 0.0 {
            let sol_metadata = self
                .get_token_info(SOLANA_NATIVE_TOKEN_ADDRESS, metadata_cache, metrics)
                .await?;
            let sol_balance_item = BalanceItem {
                name: sol_metadata.name.unwrap_or(sol_metadata.symbol.clone()),
                symbol: sol_metadata.symbol,
                chain_id: Some(SOLANA_MAINNET_CHAIN_ID.to_string()),
                address: Some(SOLANA_NATIVE_TOKEN_ADDRESS.to_string()),
                value: Some(sol_balance * sol_metadata.price),
                price: sol_metadata.price,
                quantity: BalanceQuantity {
                    decimals: sol_metadata.decimals.to_string(),
                    numeric: sol_balance.to_string(),
                },
                icon_url: sol_metadata.icon.unwrap_or_default(),
            };
            balances_vec.push(sol_balance_item);
        }

        let response = BalanceResponseBody {
            balances: balances_vec,
        };

        Ok(response)
    }

    fn provider_kind(&self) -> ProviderKind {
        self.provider_kind
    }
}

impl BalanceProviderFactory<SolScanConfig> for SolScanProvider {
    fn new(provider_config: &SolScanConfig, cache: Option<Arc<Pool>>) -> Self {
        Self {
            provider_kind: ProviderKind::SolScan,
            api_v2_token: provider_config.api_key.clone(),
            http_client: reqwest::Client::new(),
            redis_caching_pool: cache,
        }
    }
}

#[async_trait]
impl HistoryProvider for SolScanProvider {
    async fn get_transactions(
        &self,
        address: String,
        params: HistoryQueryParams,
        metadata_cache: &Arc<dyn TokenMetadataCacheProvider>,
        metrics: Arc<Metrics>,
    ) -> RpcResult<HistoryResponseBody> {
        let page_size = 100;
        let mut url =
            Url::parse(ACCOUNT_HISTORY_URL).map_err(|_| RpcError::BalanceParseURLError)?;
        url.query_pairs_mut()
            .append_pair("page_size", &page_size.to_string());
        url.query_pairs_mut().append_pair("remove_spam", "true");
        url.query_pairs_mut()
            .append_pair("exclude_amount_zero", "true");
        url.query_pairs_mut().append_pair("address", &address);
        let page = params.cursor.unwrap_or("1".into());
        url.query_pairs_mut().append_pair("page", &page);

        let latency_start = SystemTime::now();
        let response = self.send_request_v2(url).await?;
        metrics.add_latency_and_status_code_for_provider(
            &self.provider_kind,
            response.status().into(),
            latency_start,
            None,
            Some(ACCOUNT_HISTORY_URL.to_string()),
        );

        if !response.status().is_success() {
            error!(
                "Error on SolScan transactions history response. Status is not OK: {:?}",
                response.status(),
            );
            return Err(RpcError::TransactionProviderError);
        }
        let body = response.json::<HistoryResponse>().await?;

        let mut transactions: Vec<HistoryTransaction> = Vec::new();
        for item in &body.data {
            let token_info = self
                .get_token_info(&item.token_address, metadata_cache, metrics.clone())
                .await?;
            let decimal_amount = item.amount as f64 / 10f64.powf(token_info.decimals as f64);
            let transaction = HistoryTransaction {
                id: item.block_id.to_string(),
                metadata: HistoryTransactionMetadata {
                    operation_type: match item.activity_type {
                        HistoryActivityType::Transfer => {
                            if item.flow == HistoryDirectionType::In {
                                "receive".to_string()
                            } else {
                                "send".to_string()
                            }
                        }
                        HistoryActivityType::Burn => "burn".to_string(),
                        HistoryActivityType::Mint => "mint".to_string(),
                        HistoryActivityType::CreateAccount => "execute".to_string(),
                        HistoryActivityType::CloseAccount => "close".to_string(),
                    },
                    hash: item.trans_id.clone(),
                    mined_at: item.time.clone(),
                    nonce: 0,
                    sent_from: item.from_address.clone(),
                    sent_to: item.to_address.clone(),
                    status: "confirmed".to_string(), // Balance changes are always confirmed
                    application: None,
                    chain: Some(SOLANA_MAINNET_CHAIN_ID.to_string()),
                },
                transfers: Some(vec![HistoryTransactionTransfer {
                    fungible_info: Some(HistoryTransactionFungibleInfo {
                        name: token_info.name,
                        symbol: Some(token_info.symbol),
                        icon: Some(HistoryTransactionURLItem {
                            url: token_info.icon.unwrap_or_default(),
                        }),
                    }),
                    nft_info: None,
                    direction: item.flow.to_string(),
                    quantity: HistoryTransactionTransferQuantity {
                        numeric: decimal_amount.to_string(),
                    },
                    value: Some(decimal_amount * token_info.price),
                    price: Some(token_info.price),
                }]),
            };
            transactions.push(transaction);
        }

        let next = if !transactions.is_empty() && body.data.len() == page_size {
            Some((page.parse::<u64>().unwrap_or(1) + 1).to_string())
        } else {
            None
        };

        Ok(HistoryResponseBody {
            data: transactions,
            next,
        })
    }

    fn provider_kind(&self) -> ProviderKind {
        self.provider_kind
    }
}

#[async_trait]
impl FungiblePriceProvider for SolScanProvider {
    async fn get_price(
        &self,
        chain_id: &str,
        address: &str,
        currency: &SupportedCurrencies,
        metadata_cache: &Arc<dyn TokenMetadataCacheProvider>,
        metrics: Arc<Metrics>,
    ) -> RpcResult<PriceResponseBody> {
        if currency != &SupportedCurrencies::USD {
            return Err(RpcError::UnsupportedCurrency(
                "Only USD currency is supported for Solana tokens price".to_string(),
            ));
        }

        let info = self
            .get_token_info(address, metadata_cache, metrics.clone())
            .await?;
        let price = self.token_price_request(address, metrics).await?;
        let response = PriceResponseBody {
            fungibles: vec![FungiblePriceItem {
                address: format!("{}:{}:{}", CaipNamespaces::Solana, chain_id, address),
                name: info.name.unwrap_or(info.symbol.clone()),
                symbol: info.symbol,
                icon_url: info.icon.unwrap_or_default(),
                price,
                decimals: info.decimals,
            }],
        };

        Ok(response)
    }
}
