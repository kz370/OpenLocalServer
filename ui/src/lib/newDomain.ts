import type { Domain } from '@/core'

const emptyBlocks = { headers: [], redirects: [], mappings: [], upstreams: [], includes: [] }

export function newDomain(): Domain {
  return {
    hostname: '',
    project_id: null,
    root: '',
    kind: { type: 'static' },
    https: true,
    redirect_https: true,
    wildcard: false,
    enabled: true,
    ownership: 'managed',
    app: null,
    blocks: emptyBlocks,
    generated_hashes: {},
    server: null,
    path_prefix: null,
    ssl_provider: 'local_ca',
    custom_cert_path: null,
    custom_key_path: null,
    lets_encrypt_email: null,
    auto_broadcast: false,
    broadcast_provider: 'cloudflare',
  }
}
