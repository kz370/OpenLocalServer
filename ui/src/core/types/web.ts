import type { AppSpec, Ownership, SiteBlocks, SiteKind, SslProvider } from './base'
import type { Worker } from './tasks'

export interface Domain {
  hostname: string
  project_id: string | null
  root: string
  kind: SiteKind
  https: boolean
  redirect_https: boolean
  wildcard: boolean
  enabled: boolean
  ownership: Ownership
  app: AppSpec | null
  blocks: SiteBlocks
  generated_hashes: Record<string, string>
  public_domain?: string | null
  tunnel_id?: string | null
  /** Web server that renders this site: its own override, or the default. */
  server?: string | null
  /** Also served at `localhost/<path_prefix>`, alongside the site's own domain (§55). */
  path_prefix?: string | null
  /** Optional Font Awesome icon identifier or class (e.g. "fa-wordpress" or "globe"). */
  icon?: string | null
  /** SSL certificate provider: local root CA, Let's Encrypt automated ACME, or user's custom files. */
  ssl_provider?: SslProvider
  /** Absolute path to user's PEM certificate file when ssl_provider is custom. */
  custom_cert_path?: string | null
  /** Absolute path to user's PEM private key file when ssl_provider is custom. */
  custom_key_path?: string | null
  /** Optional contact email for Let's Encrypt account registration and expiry alerts. */
  lets_encrypt_email?: string | null
  /** Automatically broadcast this domain online via Cloudflare/Localtunnel/Tailscale tunnel. */
  auto_broadcast?: boolean
  /** Broadcast provider: 'cloudflare', 'localtunnel', or 'tailscale'. */
  broadcast_provider?: string | null
  /** Additional domain aliases pointing to this site. */
  aliases?: string[]
  /** Dynamic DNS provider (Cloudflare, GoDaddy, Namecheap, Duck DNS, No-IP, Porkbun, or ddclient provider). */
  ddns_provider?: string | null
  /** API token / key for Dynamic DNS updates. */
  ddns_token?: string | null
  /** Zone ID override for Cloudflare or webhook URL for custom provider. */
  ddns_zone_id?: string | null
  /** Whether to proxy DNS traffic through Cloudflare (orange cloud). */
  ddns_proxied?: boolean
  /** Last IP successfully synced with DNS provider. */
  ddns_last_ip?: string | null
  /** Timestamp of last successful DDNS sync. */
  ddns_last_synced_at?: number | null
}

export interface DdnsSyncResult {
  hostname: string
  ip: string
  updated: boolean
  message: string
}

export interface ProcfilePreview {
  source: string
  workers: Worker[]
  web: AppSpec | null
  web_port: number | null
  warnings: string[]
}
export interface DomainSummary {
  hostname: string
  url: string
  https: boolean
  kind: 'php' | 'proxy' | 'static'
  enabled: boolean
  project_id: string | null
  has_app: boolean
  folder: string
  /** Website type the site is listed under. */
  group: 'php' | 'nodejs' | 'python' | 'static' | 'proxy'
  public_domain?: string | null
  /** Web server that renders this site: its own override, or the default. */
  server: string
  /** Also served at `localhost/<path_prefix>`, alongside the site's own domain (§55). */
  path_prefix?: string | null
  /** HTTP port of the server that renders this site, for the localhost URL. */
  http_port?: number
  /** Optional Font Awesome icon identifier or class (e.g. "fa-wordpress" or "globe"). */
  icon?: string | null
  /** Data URL (`data:image/webp;base64,...`) when `ols_site_icon.webp` exists in `folder`. */
  icon_image?: string | null
  /** SSL certificate provider: local_ca, lets_encrypt, or custom. */
  ssl_provider?: SslProvider
  /** Whether the domain automatically broadcasts online. */
  auto_broadcast?: boolean
  /** Live public URL if currently broadcasted online. */
  broadcast_url?: string | null
  /** Additional domain aliases pointing to this site. */
  aliases?: string[]
  /** Dynamic DNS provider: cloudflare, duckdns, or custom. */
  ddns_provider?: string | null
  /** Last detected public IP synced to DNS. */
  ddns_last_ip?: string | null
  /** For proxy sites: the target upstream URL, e.g. "http://127.0.0.1:3000". */
  proxy_target?: string | null
}

/** Ports a server is stored with. The default server always binds 80/443 instead. */
export interface ServerPorts {
  http: number
  https: number
}
export interface WebConfig {
  default_server: string
  servers: Record<string, ServerPorts>
  php_workers: number
  dns_port: number
}
export interface PoolStatus {
  version: string
  ports: number[]
  running: boolean
}
export interface ServerAvailability {
  id: string
  name: string
  installed: boolean
  /** The server that owns 80/443 and serves every site with no override. */
  active: boolean
  running: boolean
  /** Enabled sites this server renders. Zero means it opens no listener at all. */
  sites: number
  /** Ports it will actually bind. */
  http_port: number
  https_port: number
}
export interface WebStatus {
  default_server: string
  servers: ServerAvailability[]
  running: boolean
  http_port: number
  https_port: number
  port_conflicts: string[]
  php_pools: PoolStatus[]
  apps: { hostname: string; running: boolean }[]
  dns_running: boolean
  dns_port: number
  error_log: string | null
}
export interface ApplyReport {
  server: string
  written: string[]
  unchanged: string[]
  drifted: string[]
  started: boolean
  reloaded: boolean
  hosts_updated: boolean
  validator_output: string
  warnings: string[]
}

export interface CaInfo {
  exists: boolean
  trusted: boolean
  common_name: string
  cert_path: string
}
export interface CertInfo {
  hostname: string
  sans: string[]
  issuer: string
  issued_at: number
  expires_at: number
  days_left: number
  status: 'valid' | 'expiring' | 'expired'
  trusted: boolean
  project_id: string | null
  cert_path: string
  key_path: string
  provider?: string | null
}
export interface HealthStep {
  name: string
  ok: boolean
  skipped: boolean
  detail: string
}
export interface HealthReport {
  hostname: string
  ok: boolean
  steps: HealthStep[]
}

export type ConfigPart = 'main' | 'site' | 'custom'
export interface ConfigFile {
  hostname: string | null
  part: ConfigPart
  /** Every server has its own main config, so a file needs its server to be identified. */
  server: string
  path: string
  ownership: Ownership | null
  drifted: boolean
  editable: boolean
}
export interface ConfigVersion {
  id: string
  part: ConfigPart
  timestamp_ms: number
  bytes: number
}

export interface DbUser {
  user: string
  host: string
}
export interface ConnectionInfo {
  engine: string
  host: string
  port: number | null
  user: string | null
  database: string | null
  path: string | null
  uri: string
}
export interface SqliteInfo {
  path: string
  name: string
  project_id: string | null
  exists: boolean
  size_bytes: number
  backups: string[]
}
export interface IntegrityResult {
  ok: boolean
  detail: string
}
export interface ExternalTool {
  id: string
  name: string
  engines: string[]
  executable: string
  args: string[]
}

