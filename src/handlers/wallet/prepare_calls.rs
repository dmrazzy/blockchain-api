use super::types::PreparedCalls;
use crate::analytics::MessageSource;
use crate::handlers::sessions::get::{
    get_session_context, GetSessionContextError, InternalGetSessionContextError,
};
use crate::handlers::wallet::types::SignatureRequestType;
use crate::utils::erc4337::BundlerRpcClient;
use crate::utils::erc7677::{PaymasterRpcClient, PmGetPaymasterDataParams};
use crate::{handlers::HANDLER_TASK_METRICS, state::AppState};
use alloy::primitives::{bytes, keccak256, Address, Bytes, FixedBytes, B256, U256, U64};
use alloy::providers::{Provider, ProviderBuilder};
use alloy::sol_types::SolCall;
use alloy::sol_types::SolValue;
use axum::extract::State;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;
use tracing::error;
use url::Url;
use uuid::Uuid;
use wc::future::FutureExt;
use yttrium::erc7579::accounts::safe::encode_validator_key;
use yttrium::erc7579::smart_sessions::ISmartSession::isPermissionEnabledReturn;
use yttrium::erc7579::smart_sessions::{
    enableSessionSigCall, encode_use_signature, EnableSession, ISmartSession, SmartSessionMode,
    SMART_SESSIONS_ADDRESS,
};
use yttrium::smart_accounts::account_address::AccountAddress;
use yttrium::{
    bundler::{config::BundlerConfig, pimlico::client::BundlerClient},
    chain::ChainId,
    entry_point::{EntryPointConfig, EntryPointVersion},
    smart_accounts::{nonce::get_nonce_with_key, safe::get_call_data},
    user_operation::{user_operation_hash::UserOperationHash, UserOperationV07},
};

pub type PrepareCallsRequest = Vec<PrepareCallsRequestItem>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrepareCallsRequestItem {
    from: AccountAddress,
    chain_id: U64,
    calls: Vec<CallShim>,
    capabilities: Capabilities,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CallShim {
    pub to: Address,
    pub value: U256,
    pub data: Bytes,
}

impl From<CallShim> for yttrium::call::Call {
    fn from(call: CallShim) -> Self {
        Self {
            to: call.to,
            value: call.value,
            input: call.data,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Capabilities {
    permissions: Permissions,
    paymaster_service: Option<PaymasterService>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Permissions {
    context: Uuid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymasterService {
    url: Url,
}

pub type PrepareCallsResponse = Vec<PrepareCallsResponseItem>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrepareCallsResponseItem {
    prepared_calls: PreparedCalls,
    signature_request: SignatureRequest,
    context: Uuid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignatureRequest {
    hash: UserOperationHash,
}

#[derive(Error, Debug)]
pub enum PrepareCallsError {
    #[error("Invalid address")]
    InvalidAddress,

    #[error("Invalid chain ID")]
    InvalidChainId,

    #[error("Permission not found")]
    PermissionNotFound,

    #[error("PCI not found")]
    PciNotFound,

    #[error("Permission context not long enough")]
    PermissionContextNotLongEnough,

    #[error("Permission context signature decompression error: {0}")]
    PermissionContextSignatureDecompression(fastlz_rs::DecompressError),

    #[error("Invalid permission context mode")]
    PermissionContextInvalidMode,

    #[error("Permission context ABI decode: {0}")]
    PermissionContextAbiDecode(alloy::sol_types::Error),

    #[error("Invalid permissionEnableSig for kernel account")]
    PermissionContextInvalidPermissionEnableSigForKernelAccount,

    #[error("Invalid permission context")]
    InvalidPermissionContext,

    #[error("Paymaster service capability is not supported")]
    PaymasterServiceUnsupported,

    #[error("pm_getPaymasterStubData: {0}")]
    PmGetPaymasterStubData(alloy::transports::RpcError<alloy::transports::TransportErrorKind>),

    #[error("Estimate user operation gas: {0}")]
    EstimateUserOperationGas(alloy::transports::RpcError<alloy::transports::TransportErrorKind>),

    #[error("pm_getPaymasterData: {0}")]
    PmGetPaymasterData(alloy::transports::RpcError<alloy::transports::TransportErrorKind>),

    #[error("Internal error")]
    InternalError(PrepareCallsInternalError),
}

#[derive(Error, Debug)]
pub enum PrepareCallsInternalError {
    #[error("Get nonce: {0}")]
    GetNonce(alloy::contract::Error),

    #[error("Estimate user operation gas price: {0}")]
    EstimateUserOperationGasPrice(eyre::Error),

    #[error("isSessionEnabled: {0}")]
    IsSessionEnabled(alloy::contract::Error),

    #[error("Compress session enabled: {0}")]
    CompressSessionEnabled(fastlz_rs::CompressError),

    #[error("IRN not configured")]
    IrnNotConfigured,

    #[error("Get session context: {0}")]
    GetSessionContextError(InternalGetSessionContextError),
}

impl PrepareCallsError {
    pub fn is_internal(&self) -> bool {
        matches!(self, PrepareCallsError::InternalError(_))
    }
}

pub async fn handler(
    state: State<Arc<AppState>>,
    project_id: String,
    request: PrepareCallsRequest,
) -> Result<PrepareCallsResponse, PrepareCallsError> {
    handler_internal(state, project_id, request)
        .with_metrics(HANDLER_TASK_METRICS.with_name("wallet_prepare_calls"))
        .await
}

#[tracing::instrument(skip(state), level = "debug")]
async fn handler_internal(
    state: State<Arc<AppState>>,
    project_id: String,
    request: PrepareCallsRequest,
) -> Result<PrepareCallsResponse, PrepareCallsError> {
    let mut response = Vec::with_capacity(request.len());
    for request in request {
        let chain_id = ChainId::new_eip155(request.chain_id.to::<u64>());

        // TODO check isSafe for request.from:
        // https://github.com/reown-com/web-examples/blob/32f9df464e2fa85ec49c21837d811cfe1437719e/advanced/wallets/react-wallet-v2/src/utils/UserOpBuilderUtil.ts#L39
        // What if it's not deployed yet?

        // TODO is7559Safe: https://github.com/reown-com/web-examples/blob/32f9df464e2fa85ec49c21837d811cfe1437719e/advanced/wallets/react-wallet-v2/src/lib/smart-accounts/builders/SafeUserOpBuilder.ts#L241
        // TODO shouldn't it always be 7579?

        // TODO get this from the Safe itself: https://github.com/reown-com/web-examples/blob/32f9df464e2fa85ec49c21837d811cfe1437719e/advanced/wallets/react-wallet-v2/src/lib/smart-accounts/builders/SafeUserOpBuilder.ts#L58
        // let safe_4337_module_address =

        // TODO get version from contract: https://github.com/reown-com/web-examples/blob/32f9df464e2fa85ec49c21837d811cfe1437719e/advanced/wallets/react-wallet-v2/src/lib/smart-accounts/builders/SafeUserOpBuilder.ts#L65

        let account_type = AccountType::Safe;

        // TODO run get_nonce, get gas price, and isSessionsEnabled in parallel

        let entry_point_config = EntryPointConfig {
            chain_id,
            version: EntryPointVersion::V07,
        };

        // TODO refactor to call internal proxy function directly
        let provider = ProviderBuilder::default().on_http(
            format!(
                "https://rpc.walletconnect.com/v1?chainId={}&projectId={}&source={}",
                chain_id.caip2_identifier(),
                project_id,
                MessageSource::WalletPrepareCalls,
            )
            .parse()
            .expect("Failed to parse provider URL"),
        );

        let irn_client = state.irn.as_ref().ok_or(PrepareCallsError::InternalError(
            PrepareCallsInternalError::IrnNotConfigured,
        ))?;
        let context = get_session_context(
            format!("{}:{}", chain_id.caip2_identifier(), request.from),
            request.capabilities.permissions.context,
            irn_client,
            &state.metrics,
        )
        .await
        .map_err(|e| match e {
            GetSessionContextError::PermissionNotFound(_, _) => {
                PrepareCallsError::PermissionNotFound
            }
            GetSessionContextError::InternalGetSessionContextError(e) => {
                PrepareCallsError::InternalError(PrepareCallsInternalError::GetSessionContextError(
                    e,
                ))
            }
        })?
        .ok_or(PrepareCallsError::PciNotFound)?;
        let (validator_address, signature) =
            split_permissions_context_and_check_validator(&context)?;

        // TODO refactor into yttrium
        let dummy_signature =
            get_dummy_signature(request.from, signature, account_type, &provider).await?;

        // https://github.com/reown-com/web-examples/blob/32f9df464e2fa85ec49c21837d811cfe1437719e/advanced/wallets/react-wallet-v2/src/lib/smart-accounts/builders/SafeUserOpBuilder.ts#L110
        let nonce = get_nonce_with_key(
            &provider,
            request.from,
            &entry_point_config.address(),
            encode_validator_key(validator_address),
        )
        .await
        .map_err(|e| PrepareCallsError::InternalError(PrepareCallsInternalError::GetNonce(e)))?;

        // TODO refactor to use bundler_rpc_call directly: https://github.com/WalletConnect/blockchain-api/blob/8be3ca5b08dec2387ee2c2ffcb4b7ca739443bcb/src/handlers/bundler.rs#L62
        let bundler_url = format!(
            "https://rpc.walletconnect.com/v1/bundler?chainId={}&projectId={}&bundler=pimlico",
            chain_id.caip2_identifier(),
            project_id,
        )
        .parse::<Url>()
        .expect("Failed to parse bundler URL");
        let pimlico_client = BundlerClient::new(BundlerConfig::new(bundler_url.clone()));
        let bundler_provider = BundlerRpcClient::new(bundler_url);

        // TODO cache this
        let gas_price = pimlico_client
            .estimate_user_operation_gas_price()
            .await
            .map_err(|e| {
                PrepareCallsError::InternalError(
                    PrepareCallsInternalError::EstimateUserOperationGasPrice(e),
                )
            })?;

        let user_op = UserOperationV07 {
            sender: request.from,
            nonce,
            factory: None,
            factory_data: None,
            call_data: get_call_data(request.calls.into_iter().map(|c| c.into()).collect()),
            call_gas_limit: U256::ZERO,
            verification_gas_limit: U256::ZERO,
            pre_verification_gas: U256::ZERO,
            max_fee_per_gas: gas_price.fast.max_fee_per_gas,
            max_priority_fee_per_gas: gas_price.fast.max_priority_fee_per_gas,
            paymaster: None,
            paymaster_verification_gas_limit: None,
            paymaster_post_op_gas_limit: None,
            paymaster_data: None,
            signature: dummy_signature,
        };

        let (user_op, is_final) =
            if let Some(paymaster_service) = &request.capabilities.paymaster_service {
                let paymaster_client = PaymasterRpcClient::new(paymaster_service.url.clone());

                let sponsor_user_op_result = paymaster_client
                    .pm_get_paymaster_stub_data(PmGetPaymasterDataParams {
                        user_op: user_op.clone(),
                        entrypoint: entry_point_config.address().into(),
                        chain_id: U64::from(chain_id.eip155_chain_id()),
                        context: HashMap::new(),
                    })
                    .await
                    .map_err(PrepareCallsError::PmGetPaymasterStubData)?;

                (
                    UserOperationV07 {
                        paymaster: Some(sponsor_user_op_result.paymaster),
                        paymaster_data: Some(sponsor_user_op_result.paymaster_data),
                        paymaster_verification_gas_limit: Some(
                            sponsor_user_op_result.paymaster_verification_gas_limit,
                        ),
                        paymaster_post_op_gas_limit: Some(
                            sponsor_user_op_result.paymaster_post_op_gas_limit,
                        ),
                        ..user_op
                    },
                    sponsor_user_op_result.is_final,
                )
            } else {
                (user_op, false)
            };

        let user_op = {
            let response = bundler_provider
                .eth_estimate_user_operation_gas_v07(&user_op, entry_point_config.address().into())
                .await
                .map_err(PrepareCallsError::EstimateUserOperationGas)?;

            UserOperationV07 {
                call_gas_limit: response.call_gas_limit,
                verification_gas_limit: response.verification_gas_limit,
                pre_verification_gas: response.pre_verification_gas,
                ..user_op
            }
        };

        let user_op = if let Some(paymaster_service) = request.capabilities.paymaster_service {
            if !is_final {
                let paymaster_client = PaymasterRpcClient::new(paymaster_service.url);

                let sponsor_user_op_result = paymaster_client
                    .pm_get_paymaster_data(PmGetPaymasterDataParams {
                        user_op: user_op.clone(),
                        entrypoint: entry_point_config.address().into(),
                        chain_id: U64::from(chain_id.eip155_chain_id()),
                        context: HashMap::new(),
                    })
                    .await
                    .map_err(PrepareCallsError::PmGetPaymasterData)?;

                UserOperationV07 {
                    paymaster: Some(sponsor_user_op_result.paymaster),
                    paymaster_data: Some(sponsor_user_op_result.paymaster_data),
                    ..user_op
                }
            } else {
                user_op
            }
        } else {
            user_op
        };

        let hash = user_op.hash(
            &entry_point_config.address().to_address(),
            chain_id.eip155_chain_id(),
        );

        response.push(PrepareCallsResponseItem {
            prepared_calls: PreparedCalls {
                r#type: SignatureRequestType::UserOpV7,
                data: user_op,
                chain_id: request.chain_id,
            },
            signature_request: SignatureRequest { hash },
            context: request.capabilities.permissions.context,
        });
    }

    Ok(response)
}

pub fn split_permissions_context_and_check_validator(
    context: &[u8],
) -> Result<(Address, &[u8]), PrepareCallsError> {
    let (validator_address, signature) = context
        .split_at_checked(20)
        .ok_or(PrepareCallsError::PermissionContextNotLongEnough)?;

    let validator_address = Address::from_slice(validator_address);
    if validator_address != SMART_SESSIONS_ADDRESS {
        return Err(PrepareCallsError::InvalidPermissionContext);
    }

    Ok((validator_address, signature))
}

// https://github.com/rhinestonewtf/module-sdk/blob/18ef7ca998c0d0a596572f18575e1b4967d9227b/src/account/types.ts#L4
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AccountType {
    #[serde(rename = "erc7579-implementation")]
    Erc7579Implementation,

    #[serde(rename = "kernel")]
    Kernel,

    #[serde(rename = "safe")]
    Safe,

    #[serde(rename = "nexus")]
    Nexus,
}

pub struct EnableSessionData {
    enable_session: EnableSession,
    validator: Address,
}

pub struct DecodedSmartSessionSignature {
    pub mode: SmartSessionMode,
    pub permission_id: B256,
    pub signature: Bytes,
    pub enable_session_data: Option<EnableSessionData>,
}

// https://github.com/rhinestonewtf/module-sdk/blob/18ef7ca998c0d0a596572f18575e1b4967d9227b/src/module/smart-sessions/usage.ts#L209
pub fn decode_smart_session_signature(
    signature: &[u8],
    account_type: AccountType,
) -> Result<DecodedSmartSessionSignature, PrepareCallsError> {
    let mode = signature
        .first()
        .ok_or(PrepareCallsError::PermissionContextNotLongEnough)?
        .try_into()
        .map_err(|_| PrepareCallsError::PermissionContextInvalidMode)?;

    match mode {
        SmartSessionMode::Use => {
            let permission_id: B256 = signature
                .get(1..33)
                .ok_or(PrepareCallsError::PermissionContextNotLongEnough)?
                .try_into() // this error shouldn't happen
                .map_err(|_| PrepareCallsError::PermissionContextNotLongEnough)?;
            let signature = signature
                .get(33..)
                .ok_or(PrepareCallsError::PermissionContextNotLongEnough)?
                .to_vec()
                .into();

            Ok(DecodedSmartSessionSignature {
                mode,
                permission_id,
                signature,
                enable_session_data: None, // TODO bad practice to not enforce this as part of the enum variant
            })
        }
        SmartSessionMode::Enable | SmartSessionMode::UnsafeEnable => {
            let compressed_data = signature
                .get(1..)
                .ok_or(PrepareCallsError::PermissionContextNotLongEnough)?;

            let data = fastlz_rs::decompress_to_vec(compressed_data, None)
                .map_err(PrepareCallsError::PermissionContextSignatureDecompression)?;

            let enableSessionSigCall {
                session: enable_session,
                signature,
            } = enableSessionSigCall::abi_decode_raw(&data, true)
                .map_err(PrepareCallsError::PermissionContextAbiDecode)?;
            let is_kernel = account_type == AccountType::Kernel;
            if is_kernel && enable_session.permissionEnableSig.starts_with(&[0x01]) {
                return Err(
                    PrepareCallsError::PermissionContextInvalidPermissionEnableSigForKernelAccount,
                );
            }

            let (validator, permission_enable_sig) = enable_session.permissionEnableSig
                [if is_kernel { 1 } else { 0 }..]
                .split_at_checked(20)
                .ok_or(PrepareCallsError::PermissionContextNotLongEnough)?;
            let validator = Address::from_slice(validator);
            let permission_enable_sig = permission_enable_sig.to_vec().into();

            let permission_id = keccak256(
                (
                    &enable_session.sessionToEnable.sessionValidator,
                    &enable_session.sessionToEnable.sessionValidatorInitData,
                    &enable_session.sessionToEnable.salt,
                )
                    .abi_encode_params(),
            );

            Ok(DecodedSmartSessionSignature {
                mode,
                permission_id,
                signature,
                enable_session_data: Some(EnableSessionData {
                    // enable_session,
                    enable_session: EnableSession {
                        chainDigestIndex: enable_session.chainDigestIndex,
                        hashesAndChainIds: enable_session.hashesAndChainIds,
                        sessionToEnable: enable_session.sessionToEnable,
                        permissionEnableSig: permission_enable_sig, // TODO skip all this and just pass-through as-is
                    },
                    validator,
                }),
            })
        }
    }
}

pub async fn encode_use_or_enable_smart_session_signature(
    provider: &impl Provider,
    permission_id: FixedBytes<32>,
    address: AccountAddress,
    account_type: AccountType,
    signature: Vec<u8>,
    enable_session_data: EnableSessionData,
) -> Result<Bytes, PrepareCallsError> {
    let smart_sessions = ISmartSession::new(SMART_SESSIONS_ADDRESS, provider);
    let isPermissionEnabledReturn {
        _0: session_enabled,
    } = smart_sessions
        .isPermissionEnabled(permission_id, address.to_address())
        .call()
        .await
        .map_err(|e| {
            PrepareCallsError::InternalError(PrepareCallsInternalError::IsSessionEnabled(e))
        })?;

    let signature = if session_enabled {
        encode_use_signature(permission_id, signature.into())
    } else {
        encode_enable_signature(account_type, signature, enable_session_data)?
    };

    Ok(signature)
}

fn encode_enable_signature_before_compress(
    account_type: AccountType,
    signature: Vec<u8>,
    enable_session_data: EnableSessionData,
) -> Vec<u8> {
    (
        // enable_session_data.enable_session,
        EnableSession {
            chainDigestIndex: enable_session_data.enable_session.chainDigestIndex,
            hashesAndChainIds: enable_session_data.enable_session.hashesAndChainIds,
            sessionToEnable: enable_session_data.enable_session.sessionToEnable,
            permissionEnableSig: match account_type {
                AccountType::Erc7579Implementation | AccountType::Safe | AccountType::Nexus => (
                    enable_session_data.validator,
                    enable_session_data.enable_session.permissionEnableSig,
                )
                    .abi_encode_packed()
                    .into(),
                AccountType::Kernel => (
                    [0x01],
                    enable_session_data.validator,
                    enable_session_data.enable_session.permissionEnableSig,
                )
                    .abi_encode_packed()
                    .into(),
            },
        },
        signature,
    )
        .abi_encode_params()
}

fn encode_enable_signature(
    account_type: AccountType,
    signature: Vec<u8>,
    enable_session_data: EnableSessionData,
) -> Result<Bytes, PrepareCallsError> {
    let signature =
        encode_enable_signature_before_compress(account_type, signature, enable_session_data);

    let mut compress_state = fastlz_rs::CompressState::new();
    let compressed = Bytes::from(
        compress_state
            .compress_to_vec(&signature, fastlz_rs::CompressionLevel::Default)
            .map_err(|e| {
                PrepareCallsError::InternalError(PrepareCallsInternalError::CompressSessionEnabled(
                    e,
                ))
            })?,
    );
    Ok((
        FixedBytes::from(SmartSessionMode::Enable.to_u8()),
        compressed,
    )
        .abi_encode_packed()
        .into())
}

enum SignerType {
    Ecdsa,
    Passkey,
}

fn decode_signers(data: Bytes) -> Result<Vec<SignerType>, PrepareCallsError> {
    let mut data = data.into_iter();
    let signer_count = data
        .next()
        .ok_or(PrepareCallsError::InvalidPermissionContext)?; // TODO correct error variants
    let mut signers = Vec::with_capacity(signer_count as usize);
    for _i in 0..signer_count {
        let (signer_type, length) = match data.next() {
            Some(0) => (SignerType::Ecdsa, 20),
            Some(1) => (SignerType::Passkey, 64),
            _ => return Err(PrepareCallsError::InvalidPermissionContext), // TODO correct error variants
        };
        // ignore the actual signature
        for _i in 0..length {
            data.next()
                .ok_or(PrepareCallsError::InvalidPermissionContext)?; // TODO correct error variants
        }
        signers.push(signer_type);
    }
    if data.next().is_some() {
        return Err(PrepareCallsError::InvalidPermissionContext); // TODO correct error variants
    }
    Ok(signers)
}

async fn get_dummy_signature(
    address: AccountAddress,
    signature: &[u8],
    account_type: AccountType,
    provider: &impl Provider,
) -> Result<Bytes, PrepareCallsError> {
    let DecodedSmartSessionSignature {
        mode,
        permission_id,
        signature: _,
        enable_session_data,
    } = decode_smart_session_signature(signature, account_type)?;

    assert_eq!(mode, SmartSessionMode::Enable);
    let enable_session_data = enable_session_data.unwrap();

    const DUMMY_ECDSA_SIGNATURE: Bytes = bytes!("e8b94748580ca0b4993c9a1b86b5be851bfc076ff5ce3a1ff65bf16392acfcb800f9b4f1aef1555c7fce5599fffb17e7c635502154a0333ba21f3ae491839af51c");
    const DUMMY_PASSKEY_SIGNATURE: Bytes = bytes!("00000000000000000000000000000000000000000000000000000000000000c000000000000000000000000000000000000000000000000000000000000001200000000000000000000000000000000000000000000000000000000000000001635bc6d0f68ff895cae8a288ecf7542a6a9cd555df784b73e1e2ea7e9104b1db15e9015d280cb19527881c625fee43fd3a405d5b0d199a8c8e6589a7381209e40000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000002549960de5880e8c687434170f6476605b8fe4aeb9a28632c7995cf3ba831d97631d0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000f47b2274797065223a22776562617574686e2e676574222c226368616c6c656e6765223a22746278584e465339585f3442797231634d77714b724947422d5f3330613051685a36793775634d30424f45222c226f726967696e223a22687474703a2f2f6c6f63616c686f73743a33303030222c2263726f73734f726967696e223a66616c73652c20226f746865725f6b6579735f63616e5f62655f61646465645f68657265223a22646f206e6f7420636f6d7061726520636c69656e74446174614a534f4e20616761696e737420612074656d706c6174652e205365652068747470733a2f2f676f6f2e676c2f796162506578227d000000000000000000000000");
    let signature = decode_signers(
        enable_session_data
            .enable_session
            .sessionToEnable
            .sessionValidatorInitData
            .clone(),
    )?
    .into_iter()
    .map(|t| match t {
        SignerType::Ecdsa => DUMMY_ECDSA_SIGNATURE,
        SignerType::Passkey => DUMMY_PASSKEY_SIGNATURE,
    })
    .collect::<Vec<_>>()
    .abi_encode();

    encode_use_or_enable_smart_session_signature(
        provider,
        permission_id,
        address,
        account_type,
        signature,
        enable_session_data,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy::primitives::{address, bytes, fixed_bytes};
    use yttrium::erc7579::smart_sessions::{
        ActionData, ChainDigest, ERC7739Data, PolicyData, Session,
    };

    #[test]
    fn test_encode_use_signature() {
        assert_eq!(
            encode_use_signature(
                fixed_bytes!("2ec3eb29f3b075c8fed3fb0585947b5f1ae50c2fbe2f8274918bed889f69e342"),
                bytes!("00000000000000000000000000000000000000000000000000000000000000200000000000000000000000000000000000000000000000000000000000000002000000000000000000000000000000000000000000000000000000000000004000000000000000000000000000000000000000000000000000000000000000c00000000000000000000000000000000000000000000000000000000000000041e8b94748580ca0b4993c9a1b86b5be851bfc076ff5ce3a1ff65bf16392acfcb800f9b4f1aef1555c7fce5599fffb17e7c635502154a0333ba21f3ae491839af51c000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000041e8b94748580ca0b4993c9a1b86b5be851bfc076ff5ce3a1ff65bf16392acfcb800f9b4f1aef1555c7fce5599fffb17e7c635502154a0333ba21f3ae491839af51c00000000000000000000000000000000000000000000000000000000000000")
            ),
            bytes!("002ec3eb29f3b075c8fed3fb0585947b5f1ae50c2fbe2f8274918bed889f69e34200000000000000000000000000000000000000000000000000000000000000200000000000000000000000000000000000000000000000000000000000000002000000000000000000000000000000000000000000000000000000000000004000000000000000000000000000000000000000000000000000000000000000c00000000000000000000000000000000000000000000000000000000000000041e8b94748580ca0b4993c9a1b86b5be851bfc076ff5ce3a1ff65bf16392acfcb800f9b4f1aef1555c7fce5599fffb17e7c635502154a0333ba21f3ae491839af51c000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000041e8b94748580ca0b4993c9a1b86b5be851bfc076ff5ce3a1ff65bf16392acfcb800f9b4f1aef1555c7fce5599fffb17e7c635502154a0333ba21f3ae491839af51c00000000000000000000000000000000000000000000000000000000000000"),
        );
    }

    #[test]
    fn test_encode_enable_signature_before_compress() {
        assert_eq!(
            Bytes::from(encode_enable_signature_before_compress(
                AccountType::Safe,
                bytes!("00000000000000000000000000000000000000000000000000000000000000200000000000000000000000000000000000000000000000000000000000000002000000000000000000000000000000000000000000000000000000000000004000000000000000000000000000000000000000000000000000000000000000c00000000000000000000000000000000000000000000000000000000000000041e8b94748580ca0b4993c9a1b86b5be851bfc076ff5ce3a1ff65bf16392acfcb800f9b4f1aef1555c7fce5599fffb17e7c635502154a0333ba21f3ae491839af51c000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000041e8b94748580ca0b4993c9a1b86b5be851bfc076ff5ce3a1ff65bf16392acfcb800f9b4f1aef1555c7fce5599fffb17e7c635502154a0333ba21f3ae491839af51c00000000000000000000000000000000000000000000000000000000000000").to_vec(),
                EnableSessionData {
                    enable_session: EnableSession {
                        chainDigestIndex: 0,
                        hashesAndChainIds: vec![ChainDigest {
                            chainId: 84532,
                            sessionDigest: fixed_bytes!("d921018061556bee2f63850c0762c9e7af9ad05895078ad8287f4cadc56f347a"),
                        }],
                        sessionToEnable: Session {
                            sessionValidator: address!("207b90941d9cff79A750C1E5c05dDaA17eA01B9F"),
                            sessionValidatorInitData: bytes!("020079b1cf6cb04b0e7a626c98053b3ad29d3a93527700bae0435ac2bccb87c2ef2db5e215fac4dec876f4"),
                            salt: fixed_bytes!("3100000000000000000000000000000000000000000000000000000000000000"),
                            userOpPolicies: vec![],
                            erc7739Policies: ERC7739Data {
                                allowedERC7739Content: vec![],
                                erc1271Policies: vec![],
                            },
                            actions: vec![
                                ActionData {
                                    actionTargetSelector: fixed_bytes!("efef39a1"),
                                    actionTarget: address!("2E65BAfA07238666c3b239E94F32DaD3cDD6498D"),
                                    actionPolicies: vec![
                                        PolicyData {
                                            policy: address!("9A6c4974dcE237E01Ff35c602CA9555a3c0Fa5EF"),
                                            initData: bytes!("00000000000000000000000066f8671c00000000000000000000000000000000"),
                                        }
                                    ],
                                }
                            ],
                            permitERC4337Paymaster: false,
                        },
                        permissionEnableSig: bytes!("821a568f5940148c20779e18f7fa0547c4f53f388eb684678f92774152a728a73be1f82e3f3f37a54f20e686e2a9711c280871aef1f7aa796b790ade00c0f01020"),
                    },
                    validator: address!("9388056f9cecfa536e70649154db93485a1f3448"),
                }
            )),
            bytes!("000000000000000000000000000000000000000000000000000000000000004000000000000000000000000000000000000000000000000000000000000004e00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000008000000000000000000000000000000000000000000000000000000000000000e0000000000000000000000000000000000000000000000000000000000000042000000000000000000000000000000000000000000000000000000000000000010000000000000000000000000000000000000000000000000000000000014a34d921018061556bee2f63850c0762c9e7af9ad05895078ad8287f4cadc56f347a000000000000000000000000207b90941d9cff79a750c1e5c05ddaa17ea01b9f00000000000000000000000000000000000000000000000000000000000000e031000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000140000000000000000000000000000000000000000000000000000000000000016000000000000000000000000000000000000000000000000000000000000001e00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000002b020079b1cf6cb04b0e7a626c98053b3ad29d3a93527700bae0435ac2bccb87c2ef2db5e215fac4dec876f40000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000004000000000000000000000000000000000000000000000000000000000000000600000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000010000000000000000000000000000000000000000000000000000000000000020efef39a1000000000000000000000000000000000000000000000000000000000000000000000000000000002e65bafa07238666c3b239e94f32dad3cdd6498d0000000000000000000000000000000000000000000000000000000000000060000000000000000000000000000000000000000000000000000000000000000100000000000000000000000000000000000000000000000000000000000000200000000000000000000000009a6c4974dce237e01ff35c602ca9555a3c0fa5ef0000000000000000000000000000000000000000000000000000000000000040000000000000000000000000000000000000000000000000000000000000002000000000000000000000000066f8671c0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000559388056f9cecfa536e70649154db93485a1f3448821a568f5940148c20779e18f7fa0547c4f53f388eb684678f92774152a728a73be1f82e3f3f37a54f20e686e2a9711c280871aef1f7aa796b790ade00c0f010200000000000000000000000000000000000000000000000000000000000000000000000000000000000018000000000000000000000000000000000000000000000000000000000000000200000000000000000000000000000000000000000000000000000000000000002000000000000000000000000000000000000000000000000000000000000004000000000000000000000000000000000000000000000000000000000000000c00000000000000000000000000000000000000000000000000000000000000041e8b94748580ca0b4993c9a1b86b5be851bfc076ff5ce3a1ff65bf16392acfcb800f9b4f1aef1555c7fce5599fffb17e7c635502154a0333ba21f3ae491839af51c000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000041e8b94748580ca0b4993c9a1b86b5be851bfc076ff5ce3a1ff65bf16392acfcb800f9b4f1aef1555c7fce5599fffb17e7c635502154a0333ba21f3ae491839af51c00000000000000000000000000000000000000000000000000000000000000"),
        );
    }

    #[test]
    fn test_encode_enable_signature() {
        assert_eq!(
            encode_enable_signature(
                AccountType::Safe,
                bytes!("00000000000000000000000000000000000000000000000000000000000000200000000000000000000000000000000000000000000000000000000000000002000000000000000000000000000000000000000000000000000000000000004000000000000000000000000000000000000000000000000000000000000000c00000000000000000000000000000000000000000000000000000000000000041e8b94748580ca0b4993c9a1b86b5be851bfc076ff5ce3a1ff65bf16392acfcb800f9b4f1aef1555c7fce5599fffb17e7c635502154a0333ba21f3ae491839af51c000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000041e8b94748580ca0b4993c9a1b86b5be851bfc076ff5ce3a1ff65bf16392acfcb800f9b4f1aef1555c7fce5599fffb17e7c635502154a0333ba21f3ae491839af51c00000000000000000000000000000000000000000000000000000000000000").to_vec(),
                EnableSessionData {
                    enable_session: EnableSession {
                        chainDigestIndex: 0,
                        hashesAndChainIds: vec![ChainDigest {
                            chainId: 84532,
                            sessionDigest: fixed_bytes!("64b2d184c4b8517d7f2f59bab7e6269b6aa524e268fcd1eec34a9c8e27d7389f"),
                        }],
                        sessionToEnable: Session {
                            sessionValidator: address!("207b90941d9cff79A750C1E5c05dDaA17eA01B9F"),
                            sessionValidatorInitData: bytes!("02001b60aa8eb31e11c41279f6a102026edeeb848ec600bae0435ac2bccb87c2ef2db5e215fac4dec876f4"),
                            salt: fixed_bytes!("3100000000000000000000000000000000000000000000000000000000000000"),
                            userOpPolicies: vec![],
                            erc7739Policies: ERC7739Data {
                                allowedERC7739Content: vec![],
                                erc1271Policies: vec![],
                            },
                            actions: vec![
                                ActionData {
                                    actionTargetSelector: fixed_bytes!("efef39a1"),
                                    actionTarget: address!("2E65BAfA07238666c3b239E94F32DaD3cDD6498D"),
                                    actionPolicies: vec![
                                        PolicyData {
                                            policy: address!("9A6c4974dcE237E01Ff35c602CA9555a3c0Fa5EF"),
                                            initData: bytes!("00000000000000000000000066f864d500000000000000000000000000000000"),
                                        }
                                    ],
                                }
                            ],
                            permitERC4337Paymaster: false
                        },
                        permissionEnableSig: bytes!("f0c9cba469e26f15ae4c098ff1b474b48673bb75d32e7e360391cb6e6db11c931dcc81986a86b380fcd480464b5f504fd5fa527fd9437e46ea75098adce216c81f"),
                    },
                    validator: address!("9388056f9cecfa536e70649154db93485a1f3448"),
                }
            ).unwrap(),
            bytes!("010000e015000040e0151e0104e0e0151fe018000080e01621e0165f010420e0163f0001e0141f1f014a3464b2d184c4b8517d7f2f59bab7e6269b6aa524e268fcd1eec34a9c8e2702d7389fe0033f12207b90941d9cff79a750c1e5c05ddaa17ea01be0041fe00a0001e031e00a14e021000001e1167f010160e0154b0001e1163fe018001f2b02001b60aa8eb31e11c41279f6a102026edeeb848ec600bae0435ac2bccb870bc2ef2db5e215fac4dec876f4e0184ce02a00e016ff0000e016ffe03800e2153f221203efef39a12007e01c00132e65bafa07238666c3b239e94f32dad3cdd6498de01638e017dfe0189fe0035f139a6c4974dce237e01ff35c602ca9555a3c0fa5efe0031fe00a00e1177fe0045f0366f864d5e00a43e013001f559388056f9cecfa536e70649154db93485a1f3448f0c9cba469e26f15ae4c091f8ff1b474b48673bb75d32e7e360391cb6e6db11c931dcc81986a86b380fcd48015464b5f504fd5fa527fd9437e46ea75098adce216c81fe01371e004000001e4177fe004dfe00a000002e00a13e00300e1173f00c0e0032ce00a001f41e8b94748580ca0b4993c9a1b86b5be851bfc076ff5ce3a1ff65bf16392acfc1fb800f9b4f1aef1555c7fce5599fffb17e7c635502154a0333ba21f3ae491839a01f51ce00a54e02200e0587f"),
        );
    }

    #[test]
    fn request_body_encoding_decoding() {
        let request = vec![PrepareCallsRequestItem {
            chain_id: U64::from(1),
            from: address!("207b90941d9cff79A750C1E5c05dDaA17eA01B9F").into(),
            calls: vec![CallShim {
                to: address!("207b90941d9cff79a750c1e5c05ddaa17ea01b9f"),
                data: Bytes::new(),
                value: U256::from(13),
            }],
            capabilities: Capabilities {
                permissions: Permissions {
                    context: Uuid::nil(),
                },
                paymaster_service: None,
            },
        }];
        let value = serde_json::json!([{
            "chainId": "0x1",
            "from": "0x207b90941d9cff79a750c1e5c05ddaa17ea01b9f",
            "calls": [
                {
                    "to": "0x207b90941d9cff79a750c1e5c05ddaa17ea01b9f",
                    "data": "0x",
                    "value": "0xd"
                }
            ],
            "capabilities": {
                "permissions": {
                    "context": "00000000-0000-0000-0000-000000000000",
                },
                "paymasterService": null,
            },
        }]);
        assert_eq!(serde_json::to_value(&request).unwrap(), value);
        assert_eq!(
            serde_json::from_value::<Vec<PrepareCallsRequestItem>>(value).unwrap(),
            request
        );
    }
}
