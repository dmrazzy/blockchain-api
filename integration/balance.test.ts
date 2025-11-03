import { getTestSetup } from './init';

describe('Account balance', () => {
  const { baseUrl, projectId, httpClient } = getTestSetup();

  const fulfilled_eth_address = '0x2aae531a81461f029cd55cb46703211c9227ba05'
  const fulfilled_solana_address = '5PUrktzVvJPNFYpxNzFkGp4a5Dcj1Dduif5dAzuUUhsr'

  const empty_eth_address = '0x5b6262592954B925B510651462b63ddEbcc22eaD'
  const empty_solana_address = '7ar3r6Mau1Bk7pGLWHCMj1C1bk2eCDwGWTP77j9MXTtd'

  const currency = 'usd'
  const sdk_version = '4.1.9'

  // Patch all httpClient.get calls to include the origin header
  // because the balance RPC call is not allowed without the origin header
  function withOriginHeader(options?: any) {
    const origin = 'https://rpc.walletconnect.org';
    if (!options) return { headers: { origin } };
    if (!options.headers) return { ...options, headers: { origin } };
    return { ...options, headers: { ...options.headers, origin } };
  }

  it('fulfilled balance Ethereum address', async () => {
    let resp: any = await httpClient.get(
      `${baseUrl}/v1/account/${fulfilled_eth_address}/balance?projectId=${projectId}&currency=${currency}&sv=${sdk_version}`,
      withOriginHeader()
    )
    expect(resp.status).toBe(200)
    expect(typeof resp.data.balances).toBe('object')
    expect(resp.data.balances.length).toBeGreaterThan(1)

    for (const item of resp.data.balances) {
      expect(typeof item.name).toBe('string')
      expect(typeof item.symbol).toBe('string')
      expect(item.chainId).toEqual(expect.stringMatching(/^(eip155:)?\d+$/))
      if (item.address !== undefined) {
        expect(item.address).toEqual(expect.stringMatching(/^(eip155:\d+:0x[0-9a-fA-F]{40})$/))
      } else {
        expect(item.address).toBeUndefined()
      }
      expect(typeof item.price).toBe('number')
      expect(typeof item.quantity).toBe('object')
      expect(typeof item.iconUrl).toBe('string')
    }
  })

  it('fulfilled balance Ethereum address (deprecated \'x-sdk-version\')', async () => {
    let resp: any = await httpClient.get(
      `${baseUrl}/v1/account/${fulfilled_eth_address}/balance?projectId=${projectId}&currency=${currency}`,
      withOriginHeader({
        headers: {
            'x-sdk-version': sdk_version,
        }
      })
    )
    expect(resp.status).toBe(200)
    expect(typeof resp.data.balances).toBe('object')
    expect(resp.data.balances.length).toBeGreaterThan(1)
  })

  it('fulfilled balance Solana address', async () => {
    let chainId = 'solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp'
    let resp: any = await httpClient.get(
      `${baseUrl}/v1/account/${fulfilled_solana_address}/balance?projectId=${projectId}&currency=${currency}&chainId=${chainId}&sv=${sdk_version}`,
      withOriginHeader()
    )
    expect(resp.status).toBe(200)
    expect(typeof resp.data.balances).toBe('object')
    expect(resp.data.balances.length).toBeGreaterThan(0)

    for (const item of resp.data.balances) {
      expect(item.chainId).toEqual(chainId)
      expect(typeof item.name).toBe('string')
      expect(typeof item.symbol).toBe('string')
      expect(typeof item.quantity).toBe('object')
      expect(typeof item.iconUrl).toBe('string')
    }
  })

  it('empty balance response Ethereum address: no sdk version provided', async () => {
    let resp: any = await httpClient.get(
      `${baseUrl}/v1/account/${fulfilled_eth_address}/balance?projectId=${projectId}&currency=${currency}`,
      withOriginHeader()
    )
    // We should expect the empty balance response for the sdk version prior to 4.1.9
    // that doesn't send the x-sdk-version header due to the bug in the SDK
    expect(resp.status).toBe(200)
    expect(typeof resp.data.balances).toBe('object')
    expect(resp.data.balances).toHaveLength(0)
  })

  it('empty balance response Ethereum address: no origin header', async () => {
    let resp: any = await httpClient.get(
      `${baseUrl}/v1/account/${fulfilled_eth_address}/balance?projectId=${projectId}&currency=${currency}&sv=${sdk_version}`,
    )
    // We should expect the empty balance response for an empty origin header request
    expect(resp.status).toBe(200)
    expect(typeof resp.data.balances).toBe('object')
    expect(resp.data.balances).toHaveLength(0)
  })

  it('empty balance Ethereum address affected SDK version', async () => {
    const affected_sdk_versions = ['1.6.4', '1.6.5']
    // Check for the version in query parameter
    for (const affected_sdk_version of affected_sdk_versions) {
      let resp: any = await httpClient.get(
        `${baseUrl}/v1/account/${fulfilled_eth_address}/balance?projectId=${projectId}&currency=${currency}&sv=${affected_sdk_version}`,
        withOriginHeader()
      )
      // We should expect the empty balance response for the affected SDK version
      expect(resp.status).toBe(200)
      expect(typeof resp.data.balances).toBe('object')
      expect(resp.data.balances).toHaveLength(0)
    }
    // Check for the version in header
    for (const affected_sdk_version of affected_sdk_versions) {
      let resp: any = await httpClient.get(
        `${baseUrl}/v1/account/${fulfilled_eth_address}/balance?projectId=${projectId}&currency=${currency}`,
        withOriginHeader({
          headers: {
              'x-sdk-version': `react-wagmi-${affected_sdk_version}`,
          }
        })
      )
      // We should expect the empty balance response for the affected SDK version
      expect(resp.status).toBe(200)
      expect(typeof resp.data.balances).toBe('object')
      expect(resp.data.balances).toHaveLength(0)
    }
  })

  it('empty balance Ethereum address', async () => {
    let resp: any = await httpClient.get(
      `${baseUrl}/v1/account/${empty_eth_address}/balance?projectId=${projectId}&currency=${currency}&sv=${sdk_version}`,
      withOriginHeader()
    )
    expect(resp.status).toBe(200)
    expect(typeof resp.data.balances).toBe('object')
    expect(resp.data.balances).toHaveLength(0)
  })

  it('empty balance Solana address', async () => {
    let chainId = 'solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp'
    let resp: any = await httpClient.get(
      `${baseUrl}/v1/account/${empty_solana_address}/balance?projectId=${projectId}&currency=${currency}&chainId=${chainId}&sv=${sdk_version}`,
      withOriginHeader()
    )
    expect(resp.status).toBe(200)
    expect(typeof resp.data.balances).toBe('object')
    expect(resp.data.balances).toHaveLength(0)
  })

  it('force update balance for the ERC20 token', async () => {
    // USDC token contract address on Base
    const token_contract_address = 'eip155:8453:0x833589fcd6edb6e08f4c7c32d4f71b54bda02913'
    const endpoint = `/v1/account/${fulfilled_eth_address}/balance`;
    const queryParams = `?projectId=${projectId}&currency=${currency}&sv=${sdk_version}&forceUpdate=${token_contract_address}`;
    const url = `${baseUrl}${endpoint}${queryParams}`;
    let resp = await httpClient.get(url, withOriginHeader());
    expect(resp.status).toBe(200)
    expect(typeof resp.data.balances).toBe('object')
    expect(resp.data.balances.length).toBeGreaterThan(1)

    for (const item of resp.data.balances) {
      expect(typeof item.name).toBe('string')
      expect(typeof item.symbol).toBe('string')
      expect(item.chainId).toEqual(expect.stringMatching(/^(eip155:)?\d+$/))
      if (item.address !== undefined) {
        expect(item.address).toEqual(expect.stringMatching(/^(eip155:\d+:0x[0-9a-fA-F]{40})$/))
      } else {
        expect(item.address).toBeUndefined()
      }
      expect(typeof item.price).toBe('number')
      expect(typeof item.quantity).toBe('object')
      expect(typeof item.iconUrl).toBe('string')
    }
  })

  it('force update balance for the ERC20 token (injected)', async () => {
    // Test for injected token balance if it's not in the response
    // due to the zero balance

    // Getting the empty balance without forcing balance update
    const zero_balance_address = '0x5b6262592954B925B510651462b63ddEbcc22eaD'
    const token_contract_address = 'eip155:8453:0x833589fcd6edb6e08f4c7c32d4f71b54bda02913'
    const endpoint = `/v1/account/${zero_balance_address}/balance`;
    let queryParams = `?projectId=${projectId}&currency=${currency}&sv=${sdk_version}`;
    let url = `${baseUrl}${endpoint}${queryParams}`;
    let resp = await httpClient.get(url, withOriginHeader());
    expect(resp.status).toBe(200)
    expect(typeof resp.data.balances).toBe('object')
    expect(resp.data.balances.length).toBe(0)

    // Forcing update and checking injected balance in response
    queryParams = `${queryParams}&forceUpdate=${token_contract_address}`;
    url = `${baseUrl}${endpoint}${queryParams}`;
    resp = await httpClient.get(url, withOriginHeader());
    expect(resp.status).toBe(200)
    expect(typeof resp.data.balances).toBe('object')
    expect(resp.data.balances.length).toBe(1)
    const firstItem = resp.data.balances[0]
    expect(firstItem.symbol).toBe('USDC')
    expect(firstItem.address).toBe(token_contract_address)
  })

  it('force update balance for the native Ethereum token', async () => {
    // ETH token
    // We are using `0xe...` as a contract address for native tokens
    const token_contract_address = 'eip155:1:0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee'
    const endpoint = `/v1/account/${fulfilled_eth_address}/balance`;
    const queryParams = `?projectId=${projectId}&currency=${currency}&sv=${sdk_version}&forceUpdate=${token_contract_address}`;
    const url = `${baseUrl}${endpoint}${queryParams}`;
    let resp = await httpClient.get(url, withOriginHeader());
    expect(resp.status).toBe(200)
    expect(typeof resp.data.balances).toBe('object')
    expect(resp.data.balances.length).toBeGreaterThan(1)

    for (const item of resp.data.balances) {
      expect(typeof item.name).toBe('string')
      expect(typeof item.symbol).toBe('string')
      expect(item.chainId).toEqual(expect.stringMatching(/^(eip155:)?\d+$/))
      if (item.address !== undefined) {
        expect(item.address).toEqual(expect.stringMatching(/^(eip155:\d+:0x[0-9a-fA-F]{40})$/))
      } else {
        expect(item.address).toBeUndefined()
      }
      expect(typeof item.price).toBe('number')
      expect(typeof item.quantity).toBe('object')
      expect(typeof item.iconUrl).toBe('string')
    }
  })

  it('Rootstock mainnet balance with specific tokens', async () => {
    // Test the Rootstock mainnet exception for specific tokens
    // Using an address that holds RIF, STRIF, or USDRIF tokens
    const rootstock_chain_id = 'eip155:30'
    const endpoint = `/v1/account/${fulfilled_eth_address}/balance`;
    const queryParams = `?projectId=${projectId}&currency=${currency}&chainId=${rootstock_chain_id}&sv=${sdk_version}`;
    const url = `${baseUrl}${endpoint}${queryParams}`;
    
    let resp: any = await httpClient.get(url, withOriginHeader());
    
    expect(resp.status).toBe(200)
    expect(typeof resp.data.balances).toBe('object')
    expect(Array.isArray(resp.data.balances)).toBe(true)

    // Verify the response structure for each token in the balance
    for (const item of resp.data.balances) {
      expect(typeof item.name).toBe('string')
      expect(typeof item.symbol).toBe('string')
      expect(item.chainId).toBe(rootstock_chain_id)
      
      // Verify address is in CAIP-10 format for Rootstock mainnet
      expect(item.address).toEqual(expect.stringMatching(new RegExp(`^${rootstock_chain_id}:0x[0-9a-fA-F]{40}$`)))
      
      // Verify the token is one of the expected Rootstock tokens
      const expectedTokens = [
        `${rootstock_chain_id}:0x2AcC95758f8b5F583470ba265EB685a8F45fC9D5`, // RIF
        `${rootstock_chain_id}:0x5Db91E24BD32059584bbdB831a901F1199f3D459`, // STRIF
        `${rootstock_chain_id}:0x3A15461d8aE0F0Fb5Fa2629e9DA7D66A794a6e37`, // USDRIF
      ]
      expect(expectedTokens).toContain(item.address)
      
      expect(typeof item.price).toBe('number')
      expect(typeof item.value).toBe('number')
      expect(typeof item.quantity).toBe('object')
      expect(typeof item.quantity.decimals).toBe('string')
      expect(typeof item.quantity.numeric).toBe('string')
      expect(typeof item.iconUrl).toBe('string')
    }
  })

  it('Rootstock testnet returns empty balance', async () => {
    // Verify that Rootstock testnet still returns empty balances
    const rootstock_testnet_chain_id = 'eip155:31'
    const endpoint = `/v1/account/${fulfilled_eth_address}/balance`;
    const queryParams = `?projectId=${projectId}&currency=${currency}&chainId=${rootstock_testnet_chain_id}&sv=${sdk_version}`;
    const url = `${baseUrl}${endpoint}${queryParams}`;
    
    let resp: any = await httpClient.get(url, withOriginHeader());
    
    expect(resp.status).toBe(200)
    expect(typeof resp.data.balances).toBe('object')
    expect(resp.data.balances).toHaveLength(0)
  })
})
