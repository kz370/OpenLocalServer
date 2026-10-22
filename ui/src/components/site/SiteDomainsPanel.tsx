import { open } from '@tauri-apps/plugin-dialog'
import {
  AlertCircle,
  Check,
  CheckCircle2,
  ChevronDown,
  CircleOff,
  Copy,
  Cpu,
  ExternalLink,
  Eye,
  EyeOff,
  FileCheck,
  FolderSearch,
  Globe,
  Globe2,
  Hash,
  Key,
  KeyRound,
  Layers,
  Lock,
  Network,
  Plus,
  RefreshCw,
  Save,
  Search,
  ShieldCheck,
  Trash2,
  Wifi,
  X,
  Zap,
} from 'lucide-react'
import { useEffect, useRef, useState } from 'react'

import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { FormSection, Select, Toggle } from '@/components/ui/form'
import { Input } from '@/components/ui/input'
import { type Domain, type SslProvider, runCommand } from '@/core'
import { confirmAction } from '@/lib/confirm'
import { cn } from '@/lib/utils'

export interface DdnsProviderDef {
  id: string
  name: string
  authHint: string
  category: 'Popular DNS Providers' | 'Cloud & Hosting DNS' | 'Specialized Dynamic DNS (ddclient)' | 'Custom & Standards'
  badge?: string
  keywords: string
}

export const DDNS_PROVIDERS: DdnsProviderDef[] = [
  // Popular DNS Providers
  {
    id: 'cloudflare',
    name: 'Cloudflare',
    authHint: 'API Token & Zones with optional Orange Cloud proxy',
    category: 'Popular DNS Providers',
    badge: 'Popular',
    keywords: 'cloudflare cf cdn proxy dns zones orange cloud',
  },
  {
    id: 'godaddy',
    name: 'GoDaddy',
    authHint: 'Production API Key & Secret',
    category: 'Popular DNS Providers',
    badge: 'Popular',
    keywords: 'godaddy go daddy registrar api key secret domains',
  },
  {
    id: 'namecheap',
    name: 'Namecheap',
    authHint: 'Dynamic DNS Password & Apex domain',
    category: 'Popular DNS Providers',
    badge: 'Popular',
    keywords: 'namecheap cheap domain cheapdomain dynamic dns password cpanel',
  },
  {
    id: 'duckdns',
    name: 'Duck DNS',
    authHint: 'Account Token UUID (free duckdns.org)',
    category: 'Popular DNS Providers',
    badge: 'Free',
    keywords: 'duckdns duck dns free subdomain token uuid',
  },
  {
    id: 'noip',
    name: 'No-IP',
    authHint: 'Username & Password / Update key',
    category: 'Popular DNS Providers',
    badge: 'Popular',
    keywords: 'noip no-ip noip2 ddns hostname update key',
  },
  {
    id: 'porkbun',
    name: 'Porkbun',
    authHint: 'API Key & Secret API Key',
    category: 'Popular DNS Providers',
    badge: 'Popular',
    keywords: 'porkbun pork bun api key secret registrar',
  },
  {
    id: 'digitalocean',
    name: 'DigitalOcean',
    authHint: 'Personal Access Token with DNS read/write',
    category: 'Popular DNS Providers',
    badge: 'Cloud',
    keywords: 'digitalocean digital ocean do droplet token api',
  },

  // Cloud & Hosting DNS
  {
    id: 'hetzner',
    name: 'Hetzner DNS',
    authHint: 'API Token (dns.hetzner.com)',
    category: 'Cloud & Hosting DNS',
    badge: 'Cloud',
    keywords: 'hetzner robot cloud dns zone api token',
  },
  {
    id: 'ovh',
    name: 'OVHcloud DynHost',
    authHint: 'DynHost Login & Password',
    category: 'Cloud & Hosting DNS',
    badge: 'Hosting',
    keywords: 'ovh ovhcloud dynhost dyn host telecom domain',
  },
  {
    id: 'ionos',
    name: 'IONOS (1&1)',
    authHint: 'API Key & Secret (developer.hosting.ionos.com)',
    category: 'Cloud & Hosting DNS',
    badge: 'Hosting',
    keywords: 'ionos 1&1 1and1 fasthosts api key secret hosting',
  },
  {
    id: 'gandi',
    name: 'Gandi LiveDNS',
    authHint: 'LiveDNS Personal Access Token / API Key',
    category: 'Cloud & Hosting DNS',
    badge: 'Hosting',
    keywords: 'gandi livedns live dns api key token',
  },

  // Specialized Dynamic DNS (ddclient)
  {
    id: 'freedns',
    name: 'FreeDNS (afraid.org)',
    authHint: 'Direct Update Key (from update.php?key)',
    category: 'Specialized Dynamic DNS (ddclient)',
    badge: 'ddclient',
    keywords: 'freedns afraid afraid.org update key direct url',
  },
  {
    id: 'dynu',
    name: 'Dynu',
    authHint: 'Username & IP Update Password',
    category: 'Specialized Dynamic DNS (ddclient)',
    badge: 'ddclient',
    keywords: 'dynu dynu.com username password ip update',
  },
  {
    id: 'dyndns',
    name: 'DynDNS / Dyn.com',
    authHint: 'Oracle Dyn Username & Updater Client Password',
    category: 'Specialized Dynamic DNS (ddclient)',
    badge: 'ddclient',
    keywords: 'dyndns dyn dyn.com oracle dyndns2 username password',
  },
  {
    id: 'he',
    name: 'Hurricane Electric (dns.he.net)',
    authHint: 'Dynamic DNS Key per domain / IPv6 support',
    category: 'Specialized Dynamic DNS (ddclient)',
    badge: 'ddclient',
    keywords: 'hurricane electric he he.net dns.he.net ipv6 ddns key tunnelbroker',
  },
  {
    id: 'desec',
    name: 'deSEC (dedyn.io)',
    authHint: 'Security-focused DNSSEC Token',
    category: 'Specialized Dynamic DNS (ddclient)',
    badge: 'ddclient',
    keywords: 'desec dedyn dedyn.io dnssec token privacy open source',
  },
  {
    id: 'dynv6',
    name: 'DynV6',
    authHint: 'API Token with dual-stack IPv4/IPv6 support',
    category: 'Specialized Dynamic DNS (ddclient)',
    badge: 'ddclient',
    keywords: 'dynv6 dynv6.net token api ipv4 ipv6 dualstack',
  },
  {
    id: 'cloudns',
    name: 'ClouDNS',
    authHint: 'Dynamic URL or Authentication User:Password',
    category: 'Specialized Dynamic DNS (ddclient)',
    badge: 'ddclient',
    keywords: 'cloudns anycast dynamic url user pass api',
  },
  {
    id: 'easydns',
    name: 'EasyDNS',
    authHint: 'EasyDNS Username & Password',
    category: 'Specialized Dynamic DNS (ddclient)',
    badge: 'ddclient',
    keywords: 'easydns easy dns username password',
  },
  {
    id: 'zoneedit',
    name: 'ZoneEdit',
    authHint: 'ZoneEdit Username & Dynamic Authentication Code',
    category: 'Specialized Dynamic DNS (ddclient)',
    badge: 'ddclient',
    keywords: 'zoneedit zone edit username code password',
  },
  {
    id: 'dnsexit',
    name: 'DNSExit',
    authHint: 'DNSExit Login Name & Password',
    category: 'Specialized Dynamic DNS (ddclient)',
    badge: 'ddclient',
    keywords: 'dnsexit dns exit login password',
  },
  {
    id: 'changeip',
    name: 'ChangeIP',
    authHint: 'Username & Password',
    category: 'Specialized Dynamic DNS (ddclient)',
    badge: 'ddclient',
    keywords: 'changeip change ip username password',
  },
  {
    id: 'directnic',
    name: 'Directnic',
    authHint: 'Directnic Username & Password',
    category: 'Specialized Dynamic DNS (ddclient)',
    badge: 'ddclient',
    keywords: 'directnic direct nic username password',
  },

  // Custom & Standards
  {
    id: 'custom',
    name: 'Custom Webhook / DynDNS2',
    authHint: 'Any HTTP GET/POST API or standard DynDNS2 endpoint',
    category: 'Custom & Standards',
    badge: 'Webhook',
    keywords: 'custom webhook url get post endpoint dyndns2 standards generic',
  },
]

interface SiteDomainsPanelProps {
  domain: Domain
  onSave: (updated: Domain) => Promise<void>
  busy?: boolean
}

export function SiteDomainsPanel({ domain: initialDomain, onSave, busy }: SiteDomainsPanelProps) {
  const [domain, setDomain] = useState<Domain>({
    ...initialDomain,
    aliases: initialDomain.aliases ?? [],
    ddns_provider: initialDomain.ddns_provider ?? null,
    ddns_token: initialDomain.ddns_token ?? '',
    ddns_zone_id: initialDomain.ddns_zone_id ?? '',
    ddns_proxied: initialDomain.ddns_proxied ?? false,
    ssl_provider: initialDomain.ssl_provider ?? 'local_ca',
    auto_broadcast: initialDomain.auto_broadcast ?? false,
    broadcast_provider: initialDomain.broadcast_provider ?? 'cloudflare',
  })

  const [newAlias, setNewAlias] = useState('')
  const [aliasError, setAliasError] = useState<string | null>(null)
  const [copiedDomain, setCopiedDomain] = useState<string | null>(null)
  const [existingDomains, setExistingDomains] = useState<
    { hostname: string; aliases?: string[]; public_domain?: string | null }[]
  >([])
  const [showToken, setShowToken] = useState(false)
  const [showSecret, setShowSecret] = useState(false)
  const [saving, setSaving] = useState(false)
  const [saveSuccess, setSaveSuccess] = useState(false)

  // Load existing domains on mount to validate alias uniqueness across all sites
  useEffect(() => {
    runCommand({ type: 'list_domains' })
      .then((res) => {
        if (res.type === 'domains') {
          setExistingDomains(res.domains)
        }
      })
      .catch(() => {})
  }, [])

  // Public IP detection state
  const [detectingIp, setDetectingIp] = useState(false)
  const [detectedIp, setDetectedIp] = useState<string | null>(null)

  // DDNS manual sync state
  const [syncingDdns, setSyncingDdns] = useState(false)
  const [ddnsResult, setDdnsResult] = useState<{ updated: boolean; message: string } | null>(null)

  // Let's Encrypt / Custom SSL issuance & verification state
  const [issuingSsl, setIssuingSsl] = useState(false)
  const [sslStatus, setSslStatus] = useState<string | null>(null)
  const [verifyingSsl, setVerifyingSsl] = useState(false)
  const [sslVerifyResult, setSslVerifyResult] = useState<{ valid: boolean; error: string | null } | null>(null)

  // Searchable DDNS provider state
  const [providerSearch, setProviderSearch] = useState('')
  const [isProviderSelectOpen, setIsProviderSelectOpen] = useState(false)
  const providerSelectRef = useRef<HTMLDivElement>(null)
  const searchInputRef = useRef<HTMLInputElement>(null)

  // Close provider dropdown when clicking outside or pressing Escape
  useEffect(() => {
    if (!isProviderSelectOpen) return
    const handleClickOutside = (e: MouseEvent) => {
      if (providerSelectRef.current && !providerSelectRef.current.contains(e.target as Node)) {
        setIsProviderSelectOpen(false)
      }
    }
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        setIsProviderSelectOpen(false)
      }
    }
    document.addEventListener('mousedown', handleClickOutside)
    document.addEventListener('keydown', handleKeyDown)
    return () => {
      document.removeEventListener('mousedown', handleClickOutside)
      document.removeEventListener('keydown', handleKeyDown)
    }
  }, [isProviderSelectOpen])

  // Focus search input when provider dropdown opens
  useEffect(() => {
    if (isProviderSelectOpen) {
      setTimeout(() => searchInputRef.current?.focus(), 50)
    } else {
      setProviderSearch('')
    }
  }, [isProviderSelectOpen])

  const q = providerSearch.trim().toLowerCase()
  const filteredProviders = DDNS_PROVIDERS.filter((p) => {
    if (!q) return true
    return (
      p.name.toLowerCase().includes(q) ||
      p.id.toLowerCase().includes(q) ||
      p.authHint.toLowerCase().includes(q) ||
      p.keywords.toLowerCase().includes(q) ||
      p.category.toLowerCase().includes(q)
    )
  })

  const currentProvider = DDNS_PROVIDERS.find((p) => p.id === domain.ddns_provider)

  function handleCopyDomain(text: string) {
    const url = `${domain.https ? 'https' : 'http'}://${text}`
    navigator.clipboard
      .writeText(url)
      .then(() => {
        setCopiedDomain(text)
        setTimeout(() => setCopiedDomain(null), 1500)
      })
      .catch(() => {
        navigator.clipboard.writeText(text).then(() => {
          setCopiedDomain(text)
          setTimeout(() => setCopiedDomain(null), 1500)
        })
      })
  }

  function handleOpenDomain(text: string) {
    const url = `${domain.https ? 'https' : 'http'}://${text}`
    runCommand({ type: 'open_url', url }).catch(() => {})
  }

  function addAlias() {
    const clean = newAlias.trim().toLowerCase()
    if (!clean) return
    if (clean === domain.hostname.toLowerCase()) {
      setAliasError('Alias cannot be identical to the primary domain.')
      return
    }
    if (domain.public_domain && clean === domain.public_domain.toLowerCase()) {
      setAliasError('Alias cannot be identical to the public domain.')
      return
    }
    const current = domain.aliases ?? []
    if (current.map((a) => a.toLowerCase()).includes(clean)) {
      setAliasError('This alias is already added to this site.')
      return
    }

    // Uniqueness check: verify no other site owns this hostname or alias
    const conflictSite = existingDomains.find((other) => {
      if (other.hostname.toLowerCase() === domain.hostname.toLowerCase()) return false
      if (other.hostname.toLowerCase() === clean) return true
      if (other.public_domain?.toLowerCase() === clean) return true
      if ((other.aliases ?? []).some((a) => a.toLowerCase() === clean)) return true
      return false
    })

    if (conflictSite) {
      setAliasError(
        `The alias "${clean}" is already in use by site "${conflictSite.hostname}". Each alias must be globally unique.`
      )
      return
    }

    setAliasError(null)
    setDomain({ ...domain, aliases: [...current, clean] })
    setNewAlias('')
  }

  async function removeAlias(index: number) {
    const current = domain.aliases ?? []
    const aliasToRemove = current[index]
    if (!aliasToRemove) return
    const confirmed = await confirmAction(
      `Are you sure you want to delete the alias "${aliasToRemove}"? The web server will no longer route requests for this domain to this site.`,
      'Delete Domain Alias',
      'Delete'
    )
    if (!confirmed) return
    setDomain({ ...domain, aliases: current.filter((_, i) => i !== index) })
  }

  async function handleDetectPublicIp() {
    setDetectingIp(true)
    try {
      const res = await runCommand({ type: 'detect_public_ip' })
      if (res.type === 'public_ip') {
        setDetectedIp(res.ip)
      }
    } catch (e: any) {
      setDetectedIp(`Detection failed: ${e?.message ?? e}`)
    } finally {
      setDetectingIp(false)
    }
  }

  async function handleSyncDdns(force: boolean) {
    if (!domain.ddns_provider || !domain.ddns_token?.trim()) {
      setDdnsResult({
        updated: false,
        message: 'Please select a DDNS provider and configure your API credentials first.',
      })
      return
    }
    setSyncingDdns(true)
    setDdnsResult(null)
    try {
      await onSave(domain)
      const res = await runCommand({
        type: 'sync_ddns',
        hostname: domain.hostname,
        force,
      })
      if (res.type === 'ddns_sync') {
        setDdnsResult({
          updated: res.result.updated,
          message: res.result.message,
        })
        setDomain((prev) => ({
          ...prev,
          ddns_last_ip: res.result.ip,
          ddns_last_synced_at: Math.floor(Date.now() / 1000),
        }))
      }
    } catch (e: any) {
      const detail = e?.cause ? `${e?.problem ?? 'Sync failed'}: ${e.cause}` : (e?.problem ?? e?.message ?? e)
      setDdnsResult({
        updated: false,
        message: `Sync failed: ${detail}`,
      })
    } finally {
      setSyncingDdns(false)
    }
  }

  async function handleIssueLetsEncrypt() {
    setIssuingSsl(true)
    setSslStatus('Requesting ACME HTTP-01 challenge from Let\'s Encrypt…')
    try {
      await onSave(domain)
      const res = await runCommand({
        type: 'issue_lets_encrypt',
        hostname: domain.hostname,
        email: domain.lets_encrypt_email,
      })
      if (res.type === 'certificate') {
        setSslStatus(`Certificate issued by ${res.cert.issuer}. Valid for ${res.cert.days_left} days.`)
      }
    } catch (e: any) {
      setSslStatus(`Error: ${e?.cause ?? e?.message ?? e}`)
    } finally {
      setIssuingSsl(false)
    }
  }

  async function handleVerifyCustomSsl() {
    if (!domain.custom_cert_path || !domain.custom_key_path) return
    setVerifyingSsl(true)
    setSslVerifyResult(null)
    try {
      const resp = await runCommand({
        type: 'verify_ssl_files',
        cert_path: domain.custom_cert_path,
        key_path: domain.custom_key_path,
      })
      if (resp.type === 'ssl_verification') {
        setSslVerifyResult({ valid: resp.valid, error: resp.error })
      }
    } catch (e) {
      setSslVerifyResult({ valid: false, error: String(e) })
    } finally {
      setVerifyingSsl(false)
    }
  }

  async function handleSave() {
    setSaving(true)
    setSaveSuccess(false)
    setAliasError(null)
    try {
      await onSave(domain)
      setSaveSuccess(true)
      setTimeout(() => setSaveSuccess(false), 3000)

      // Automatically trigger background DDNS sync if configured
      if (domain.ddns_provider && domain.ddns_provider !== 'disabled' && domain.ddns_token?.trim()) {
        runCommand({
          type: 'sync_ddns',
          hostname: domain.hostname,
          force: false,
        })
          .then((res) => {
            if (res.type === 'ddns_sync') {
              setDdnsResult({
                updated: res.result.updated,
                message: res.result.message,
              })
              if (res.result.ip) {
                setDomain((prev) => ({
                  ...prev,
                  ddns_last_ip: res.result.ip,
                  ddns_last_synced_at: Math.floor(Date.now() / 1000),
                }))
              }
            }
          })
          .catch((e: any) => {
            const detail = e?.cause ? `${e?.problem ?? 'Sync failed'}: ${e.cause}` : (e?.problem ?? e?.message ?? e)
            setDdnsResult({
              updated: false,
              message: `Sync failed: ${detail}`,
            })
          })
      }
    } catch (e: any) {
      const msg = e?.cause ? `${e?.problem ?? 'Save failed'}: ${e.cause}` : (e?.problem ?? e?.message ?? e)
      setAliasError(msg)
    } finally {
      setSaving(false)
    }
  }

  const isDdnsEnabled = !!domain.ddns_provider && domain.ddns_provider !== 'disabled'

  return (
    <div className="flex flex-col gap-6 max-w-4xl">
      {/* 1. Primary Domain & Aliases */}
      <FormSection
        title="Domains & Aliases"
        hint="Configure multiple domain names and aliases pointing to this site with automatic multi-SAN certificates."
      >
        <div className="flex flex-col gap-3.5">
          {/* Primary Domain Card */}
          <div className="flex items-center justify-between rounded-lg border border-border/80 bg-card p-3.5 shadow-xs">
            <div className="flex items-center gap-3 min-w-0">
              <div className="flex size-9 items-center justify-center rounded-lg bg-sky-500/10 text-sky-500 shrink-0">
                <Globe className="size-5" />
              </div>
              <div className="min-w-0">
                <div className="flex items-center gap-2">
                  <span className="font-semibold text-sm truncate">{domain.hostname}</span>
                  <Badge variant="secondary" className="text-[10px] uppercase font-bold tracking-wider shrink-0">
                    Primary
                  </Badge>
                </div>
                <p className="text-xs text-muted-foreground truncate">Main root domain served by your web server.</p>
              </div>
            </div>
            <div className="flex items-center gap-1.5 shrink-0 ml-2">
              <Button
                type="button"
                variant="ghost"
                size="sm"
                onClick={() => handleCopyDomain(domain.hostname)}
                className="h-8 px-2.5 text-xs text-muted-foreground hover:text-foreground gap-1.5 cursor-pointer"
                title="Copy primary domain URL"
              >
                {copiedDomain === domain.hostname ? (
                  <>
                    <Check className="size-3.5 text-emerald-500" />
                    <span className="text-emerald-500 text-xs font-medium">Copied!</span>
                  </>
                ) : (
                  <>
                    <Copy className="size-3.5" />
                    <span>Copy</span>
                  </>
                )}
              </Button>
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={() => handleOpenDomain(domain.hostname)}
                className="h-8 px-2.5 text-xs gap-1.5 cursor-pointer"
                title={`Open ${domain.https ? 'https' : 'http'}://${domain.hostname}`}
              >
                <ExternalLink className="size-3.5" />
                <span>Open</span>
              </Button>
            </div>
          </div>

          {/* Aliases List */}
          <div className="flex flex-col gap-2">
            <div className="flex items-center justify-between">
              <label className="text-xs font-semibold uppercase tracking-wider text-muted-foreground flex items-center gap-1.5">
                <Layers className="size-3.5 text-sky-400" />
                Extra Domain Aliases ({(domain.aliases ?? []).length})
              </label>
              <span className="text-[11px] text-muted-foreground">Each alias receives virtual host routing & SAN SSL</span>
            </div>

            {(domain.aliases ?? []).length === 0 ? (
              <p className="text-xs text-muted-foreground italic py-1 px-1">
                No extra aliases configured. Add additional domain names below.
              </p>
            ) : (
              <div className="flex flex-col gap-1.5">
                {(domain.aliases ?? []).map((alias, idx) => (
                  <div
                    key={idx}
                    className="flex items-center justify-between rounded-md border border-border bg-card px-3 py-2 text-sm hover:border-border/80 transition-colors"
                  >
                    <div className="flex items-center gap-2.5 min-w-0">
                      <Globe2 className="size-3.5 text-sky-400 shrink-0" />
                      <span className="font-mono text-xs truncate">{alias}</span>
                    </div>
                    <div className="flex items-center gap-1 shrink-0 ml-2">
                      <Button
                        type="button"
                        variant="ghost"
                        size="sm"
                        onClick={() => handleCopyDomain(alias)}
                        className="h-7 px-2 text-xs text-muted-foreground hover:text-foreground gap-1 cursor-pointer"
                        title="Copy alias URL"
                      >
                        {copiedDomain === alias ? (
                          <>
                            <Check className="size-3 text-emerald-500" />
                            <span className="text-emerald-500 text-[11px] font-medium">Copied!</span>
                          </>
                        ) : (
                          <>
                            <Copy className="size-3" />
                            <span className="text-[11px]">Copy</span>
                          </>
                        )}
                      </Button>
                      <Button
                        type="button"
                        variant="ghost"
                        size="sm"
                        onClick={() => handleOpenDomain(alias)}
                        className="h-7 px-2 text-xs text-muted-foreground hover:text-foreground gap-1 cursor-pointer"
                        title={`Open ${domain.https ? 'https' : 'http'}://${alias}`}
                      >
                        <ExternalLink className="size-3" />
                        <span className="text-[11px]">Open</span>
                      </Button>
                      <Button
                        type="button"
                        variant="ghost"
                        size="sm"
                        onClick={() => removeAlias(idx)}
                        className="size-7 p-0 text-muted-foreground hover:text-destructive cursor-pointer"
                        title="Remove alias"
                      >
                        <Trash2 className="size-3.5" />
                      </Button>
                    </div>
                  </div>
                ))}
              </div>
            )}

            {/* Add Alias Input */}
            <div className="flex gap-2 mt-1">
              <div className="relative flex-1">
                <Globe className="absolute left-3 top-1/2 -translate-y-1/2 size-3.5 text-muted-foreground" />
                <Input
                  placeholder="e.g. api.example.com, admin.example.com, mydomain.org"
                  value={newAlias}
                  onChange={(e) => {
                    setNewAlias(e.target.value)
                    if (aliasError) setAliasError(null)
                  }}
                  onKeyDown={(e) => {
                    if (e.key === 'Enter') {
                      e.preventDefault()
                      addAlias()
                    }
                  }}
                  className="pl-9 text-xs font-mono"
                />
              </div>
              <Button type="button" variant="secondary" size="sm" onClick={addAlias} className="shrink-0 gap-1.5">
                <Plus className="size-3.5" /> Add Alias
              </Button>
            </div>
            {aliasError && <p className="text-xs text-destructive">{aliasError}</p>}
          </div>

          {/* Wildcard Subdomains */}
          <div className="pt-1 border-t border-border/50">
            <Toggle
              checked={domain.wildcard}
              onChange={(wildcard) => setDomain({ ...domain, wildcard })}
              label={`Accept wildcard subdomains (*.${domain.hostname})`}
              hint="Routes any unassigned subdomain to this site automatically."
            />
          </div>
        </div>
      </FormSection>

      {/* 2. Dynamic DNS (DDNS) Configuration */}
      <FormSection
        title="Dynamic DNS (DDNS)"
        hint="Automatically updates DNS records with your DNS provider only when your server's public IP changes."
      >
        <div className="flex flex-col gap-4">
          {/* Searchable Provider Combobox */}
          <div ref={providerSelectRef} className="relative flex flex-col gap-1.5">
            <label className="text-xs font-medium flex items-center justify-between">
              <span className="flex items-center gap-1.5">
                <Network className="size-4 text-emerald-400" />
                Dynamic DNS Provider
              </span>
              <span className="text-[11px] text-muted-foreground">Supported by ddclient & native APIs</span>
            </label>

            {/* Trigger Button */}
            <button
              type="button"
              onClick={() => setIsProviderSelectOpen(!isProviderSelectOpen)}
              className={cn(
                'flex h-9 w-full items-center justify-between rounded-lg border bg-input/60 px-3.5 text-xs transition-colors cursor-pointer',
                'hover:bg-input hover:border-ring/50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/30',
                isProviderSelectOpen ? 'border-ring bg-background' : 'border-border'
              )}
              aria-haspopup="listbox"
              aria-expanded={isProviderSelectOpen}
            >
              <div className="flex items-center gap-2 min-w-0">
                {currentProvider ? (
                  <>
                    <Globe2 className="size-4 text-primary shrink-0" />
                    <span className="font-medium text-foreground truncate">{currentProvider.name}</span>
                    <span className="text-muted-foreground truncate hidden sm:inline text-[11px]">
                      — {currentProvider.authHint}
                    </span>
                  </>
                ) : (
                  <>
                    <CircleOff className="size-4 text-muted-foreground shrink-0" />
                    <span className="text-muted-foreground">Disabled — No Dynamic DNS</span>
                  </>
                )}
              </div>
              <div className="flex items-center gap-1.5 shrink-0 ml-2">
                {currentProvider?.badge && (
                  <Badge variant="outline" className="text-[10px] px-1.5 py-0 border-primary/30 text-primary">
                    {currentProvider.badge}
                  </Badge>
                )}
                <ChevronDown
                  className={cn(
                    'size-4 text-muted-foreground transition-transform duration-200',
                    isProviderSelectOpen && 'rotate-180'
                  )}
                />
              </div>
            </button>

            {/* Searchable Dropdown Menu Popover */}
            {isProviderSelectOpen && (
              <div className="absolute top-full left-0 right-0 z-50 mt-1.5 flex flex-col rounded-lg border border-border bg-popover/95 backdrop-blur-md p-2 shadow-2xl animate-in fade-in-50 zoom-in-95 duration-150">
                {/* Search Input Bar */}
                <div className="relative mb-2">
                  <Search className="pointer-events-none absolute left-2.5 top-1/2 size-3.5 -translate-y-1/2 text-muted-foreground" />
                  <Input
                    ref={searchInputRef}
                    value={providerSearch}
                    onChange={(e) => setProviderSearch(e.target.value)}
                    placeholder="Search 20+ providers (e.g. GoDaddy, Namecheap, Cloudflare, OVH)..."
                    className="h-8 pl-8 pr-7 text-xs"
                  />
                  {providerSearch && (
                    <button
                      type="button"
                      onClick={() => setProviderSearch('')}
                      className="absolute right-2 top-1/2 -translate-y-1/2 text-muted-foreground hover:text-foreground cursor-pointer"
                    >
                      <X className="size-3.5" />
                    </button>
                  )}
                </div>

                {/* Options List */}
                <div className="flex max-h-64 flex-col gap-0.5 overflow-y-auto pr-1">
                  {/* Disabled Option */}
                  {(!q || 'disabled no dynamic dns none off'.includes(q)) && (
                    <button
                      type="button"
                      onClick={() => {
                        setDomain({ ...domain, ddns_provider: null })
                        setIsProviderSelectOpen(false)
                      }}
                      className={cn(
                        'flex w-full items-center justify-between rounded-md px-2.5 py-2 text-left text-xs transition-colors cursor-pointer',
                        !domain.ddns_provider
                          ? 'bg-primary/10 text-primary font-medium'
                          : 'hover:bg-accent/60 text-muted-foreground'
                      )}
                    >
                      <div className="flex items-center gap-2">
                        <CircleOff className="size-3.5 shrink-0" />
                        <span>Disabled — No Dynamic DNS</span>
                      </div>
                      {!domain.ddns_provider && <Check className="size-3.5 text-primary" />}
                    </button>
                  )}

                  {/* Filtered Provider Items */}
                  {filteredProviders.length === 0 ? (
                    <div className="flex flex-col items-center justify-center py-6 text-center">
                      <p className="text-xs text-muted-foreground">No DNS providers match "{providerSearch}"</p>
                      <button
                        type="button"
                        onClick={() => {
                          setDomain({ ...domain, ddns_provider: 'custom' })
                          setIsProviderSelectOpen(false)
                        }}
                        className="mt-2 text-xs font-medium text-primary hover:underline cursor-pointer"
                      >
                        Use Custom Webhook / DynDNS2 instead →
                      </button>
                    </div>
                  ) : (
                    (['Popular DNS Providers', 'Cloud & Hosting DNS', 'Specialized Dynamic DNS (ddclient)', 'Custom & Standards'] as const).map(
                      (category) => {
                        const items = filteredProviders.filter((p) => p.category === category)
                        if (items.length === 0) return null
                        return (
                          <div key={category} className="flex flex-col">
                            <div className="sticky top-0 bg-popover/90 backdrop-blur-sm px-2.5 py-1 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground/80">
                              {category}
                            </div>
                            {items.map((p) => {
                              const isSelected = domain.ddns_provider === p.id
                              return (
                                <button
                                  key={p.id}
                                  type="button"
                                  onClick={() => {
                                    setDomain({ ...domain, ddns_provider: p.id })
                                    setIsProviderSelectOpen(false)
                                  }}
                                  className={cn(
                                    'flex w-full items-center justify-between rounded-md px-2.5 py-2 text-left text-xs transition-colors cursor-pointer',
                                    isSelected
                                      ? 'bg-primary/10 text-primary font-medium'
                                      : 'hover:bg-accent/60 text-foreground'
                                  )}
                                >
                                  <div className="flex flex-col gap-0.5 min-w-0 pr-2">
                                    <div className="flex items-center gap-2">
                                      <span className="font-medium truncate">{p.name}</span>
                                      {p.badge && (
                                        <span className="rounded bg-muted px-1.5 py-0.5 text-[9px] text-muted-foreground shrink-0 font-normal">
                                          {p.badge}
                                        </span>
                                      )}
                                    </div>
                                    <span className="text-[11px] text-muted-foreground truncate">{p.authHint}</span>
                                  </div>
                                  {isSelected && <Check className="size-3.5 text-primary shrink-0" />}
                                </button>
                              )
                            })}
                          </div>
                        )
                      }
                    )
                  )}
                </div>
              </div>
            )}
          </div>

          {/* Provider Specific Settings */}
          {isDdnsEnabled && (
            <div className="flex flex-col gap-3.5 rounded-lg border border-border bg-accent/20 p-4">
              {/* Cloudflare Options */}
              {domain.ddns_provider === 'cloudflare' && (
                <>
                  <div className="flex flex-col gap-1.5">
                    <label className="text-xs font-medium flex items-center gap-1.5">
                      <Key className="size-3.5 text-muted-foreground" />
                      Cloudflare API Token
                    </label>
                    <div className="relative">
                      <Input
                        type={showToken ? 'text' : 'password'}
                        placeholder="Cloudflare API token with Zone.DNS permissions"
                        value={domain.ddns_token ?? ''}
                        onChange={(e) => setDomain({ ...domain, ddns_token: e.target.value })}
                        className="pr-10 text-xs font-mono"
                      />
                      <button
                        type="button"
                        onClick={() => setShowToken(!showToken)}
                        className="absolute right-2.5 top-1/2 -translate-y-1/2 text-muted-foreground hover:text-foreground"
                      >
                        {showToken ? <EyeOff className="size-3.5" /> : <Eye className="size-3.5" />}
                      </button>
                    </div>
                    <p className="text-[11px] text-muted-foreground">
                      Create a token in Cloudflare Dashboard → My Profile → API Tokens with <code className="bg-muted px-1 py-0.5 rounded">Zone.DNS:Edit</code> permissions.
                    </p>
                  </div>

                  <div className="flex flex-col gap-1.5">
                    <label className="text-xs font-medium flex items-center gap-1.5">
                      <Hash className="size-3.5 text-muted-foreground" />
                      Zone ID (Optional)
                    </label>
                    <Input
                      placeholder="Leave blank for automatic apex zone resolution"
                      value={domain.ddns_zone_id ?? ''}
                      onChange={(e) => setDomain({ ...domain, ddns_zone_id: e.target.value })}
                      className="text-xs font-mono"
                    />
                    <p className="text-[11px] text-muted-foreground">
                      If left empty, OLS automatically resolves the apex zone from the domain hostname.
                    </p>
                  </div>

                  <div className="pt-1">
                    <Toggle
                      checked={domain.ddns_proxied ?? false}
                      onChange={(proxied) => setDomain({ ...domain, ddns_proxied: proxied })}
                      label="Cloudflare Proxy (Orange Cloud)"
                      hint="Route traffic through Cloudflare's CDN and DDoS protection proxy."
                    />
                  </div>
                </>
              )}

              {/* GoDaddy Options */}
              {domain.ddns_provider === 'godaddy' && (
                <>
                  <div className="flex flex-col gap-1.5">
                    <label className="text-xs font-medium flex items-center gap-1.5">
                      <Key className="size-3.5 text-muted-foreground" />
                      GoDaddy API Key
                    </label>
                    <Input
                      placeholder="GoDaddy API Key (from developer.godaddy.com/keys)"
                      value={domain.ddns_token ?? ''}
                      onChange={(e) => setDomain({ ...domain, ddns_token: e.target.value })}
                      className="text-xs font-mono"
                    />
                  </div>
                  <div className="flex flex-col gap-1.5">
                    <label className="text-xs font-medium flex items-center gap-1.5">
                      <Lock className="size-3.5 text-muted-foreground" />
                      GoDaddy API Secret
                    </label>
                    <div className="relative">
                      <Input
                        type={showSecret ? 'text' : 'password'}
                        placeholder="GoDaddy API Secret"
                        value={domain.ddns_zone_id ?? ''}
                        onChange={(e) => setDomain({ ...domain, ddns_zone_id: e.target.value })}
                        className="pr-10 text-xs font-mono"
                      />
                      <button
                        type="button"
                        onClick={() => setShowSecret(!showSecret)}
                        className="absolute right-2.5 top-1/2 -translate-y-1/2 text-muted-foreground hover:text-foreground"
                      >
                        {showSecret ? <EyeOff className="size-3.5" /> : <Eye className="size-3.5" />}
                      </button>
                    </div>
                    <p className="text-[11px] text-muted-foreground">
                      Obtain your Production API Key & Secret from <a href="https://developer.godaddy.com/keys" target="_blank" rel="noreferrer" className="text-primary underline">developer.godaddy.com/keys</a>.
                    </p>
                  </div>
                </>
              )}

              {/* Namecheap Options */}
              {domain.ddns_provider === 'namecheap' && (
                <>
                  <div className="flex flex-col gap-1.5">
                    <label className="text-xs font-medium flex items-center gap-1.5">
                      <Lock className="size-3.5 text-muted-foreground" />
                      Namecheap Dynamic DNS Password
                    </label>
                    <div className="relative">
                      <Input
                        type={showToken ? 'text' : 'password'}
                        placeholder="Dynamic DNS Password from Namecheap cPanel"
                        value={domain.ddns_token ?? ''}
                        onChange={(e) => setDomain({ ...domain, ddns_token: e.target.value })}
                        className="pr-10 text-xs font-mono"
                      />
                      <button
                        type="button"
                        onClick={() => setShowToken(!showToken)}
                        className="absolute right-2.5 top-1/2 -translate-y-1/2 text-muted-foreground hover:text-foreground"
                      >
                        {showToken ? <EyeOff className="size-3.5" /> : <Eye className="size-3.5" />}
                      </button>
                    </div>
                    <p className="text-[11px] text-muted-foreground">
                      In Namecheap Dashboard: Domain List → Manage → Advanced DNS → Dynamic DNS → copy your Password.
                    </p>
                  </div>
                  <div className="flex flex-col gap-1.5">
                    <label className="text-xs font-medium flex items-center gap-1.5">
                      <Globe2 className="size-3.5 text-muted-foreground" />
                      Apex Domain Override (Optional)
                    </label>
                    <Input
                      placeholder="Leave blank to auto-detect apex domain"
                      value={domain.ddns_zone_id ?? ''}
                      onChange={(e) => setDomain({ ...domain, ddns_zone_id: e.target.value })}
                      className="text-xs font-mono"
                    />
                  </div>
                </>
              )}

              {/* DuckDNS Options */}
              {domain.ddns_provider === 'duckdns' && (
                <div className="flex flex-col gap-1.5">
                  <label className="text-xs font-medium flex items-center gap-1.5">
                    <Key className="size-3.5 text-muted-foreground" />
                    DuckDNS Account Token
                  </label>
                  <div className="relative">
                    <Input
                      type={showToken ? 'text' : 'password'}
                      placeholder="Your DuckDNS Account Token UUID"
                      value={domain.ddns_token ?? ''}
                      onChange={(e) => setDomain({ ...domain, ddns_token: e.target.value })}
                      className="pr-10 text-xs font-mono"
                    />
                    <button
                      type="button"
                      onClick={() => setShowToken(!showToken)}
                      className="absolute right-2.5 top-1/2 -translate-y-1/2 text-muted-foreground hover:text-foreground"
                    >
                      {showToken ? <EyeOff className="size-3.5" /> : <Eye className="size-3.5" />}
                    </button>
                  </div>
                  <p className="text-[11px] text-muted-foreground">
                    DuckDNS subdomain is extracted from your hostname (e.g. <code>myproject.duckdns.org</code>).
                  </p>
                </div>
              )}

              {/* Porkbun Options */}
              {domain.ddns_provider === 'porkbun' && (
                <>
                  <div className="flex flex-col gap-1.5">
                    <label className="text-xs font-medium flex items-center gap-1.5">
                      <Key className="size-3.5 text-muted-foreground" />
                      Porkbun API Key
                    </label>
                    <Input
                      placeholder="pk1_..."
                      value={domain.ddns_token ?? ''}
                      onChange={(e) => setDomain({ ...domain, ddns_token: e.target.value })}
                      className="text-xs font-mono"
                    />
                  </div>
                  <div className="flex flex-col gap-1.5">
                    <label className="text-xs font-medium flex items-center gap-1.5">
                      <Lock className="size-3.5 text-muted-foreground" />
                      Porkbun Secret API Key
                    </label>
                    <div className="relative">
                      <Input
                        type={showSecret ? 'text' : 'password'}
                        placeholder="sk1_..."
                        value={domain.ddns_zone_id ?? ''}
                        onChange={(e) => setDomain({ ...domain, ddns_zone_id: e.target.value })}
                        className="pr-10 text-xs font-mono"
                      />
                      <button
                        type="button"
                        onClick={() => setShowSecret(!showSecret)}
                        className="absolute right-2.5 top-1/2 -translate-y-1/2 text-muted-foreground hover:text-foreground"
                      >
                        {showSecret ? <EyeOff className="size-3.5" /> : <Eye className="size-3.5" />}
                      </button>
                    </div>
                  </div>
                </>
              )}

              {/* DigitalOcean / Hetzner / Gandi / Token-based providers */}
              {['digitalocean', 'hetzner', 'gandi', 'desec', 'dynv6'].includes(domain.ddns_provider ?? '') && (
                <div className="flex flex-col gap-1.5">
                  <label className="text-xs font-medium flex items-center gap-1.5">
                    <Key className="size-3.5 text-muted-foreground" />
                    API Access Token
                  </label>
                  <div className="relative">
                    <Input
                      type={showToken ? 'text' : 'password'}
                      placeholder="Personal Access Token or API Key"
                      value={domain.ddns_token ?? ''}
                      onChange={(e) => setDomain({ ...domain, ddns_token: e.target.value })}
                      className="pr-10 text-xs font-mono"
                    />
                    <button
                      type="button"
                      onClick={() => setShowToken(!showToken)}
                      className="absolute right-2.5 top-1/2 -translate-y-1/2 text-muted-foreground hover:text-foreground"
                    >
                      {showToken ? <EyeOff className="size-3.5" /> : <Eye className="size-3.5" />}
                    </button>
                  </div>
                </div>
              )}

              {/* Username & Password based providers (No-IP, DynDNS, Dynu, OVH, EasyDNS, etc.) */}
              {['noip', 'dyndns', 'dynu', 'he', 'ovh', 'ionos', 'easydns', 'zoneedit', 'dnsexit', 'changeip', 'directnic'].includes(
                domain.ddns_provider ?? ''
              ) && (
                <>
                  <div className="flex flex-col gap-1.5">
                    <label className="text-xs font-medium flex items-center gap-1.5">
                      <Key className="size-3.5 text-muted-foreground" />
                      Username / Account Key
                    </label>
                    <Input
                      placeholder="Account Username, Email or Key"
                      value={domain.ddns_token ?? ''}
                      onChange={(e) => setDomain({ ...domain, ddns_token: e.target.value })}
                      className="text-xs font-mono"
                    />
                  </div>
                  <div className="flex flex-col gap-1.5">
                    <label className="text-xs font-medium flex items-center gap-1.5">
                      <Lock className="size-3.5 text-muted-foreground" />
                      Password / Update Secret
                    </label>
                    <div className="relative">
                      <Input
                        type={showSecret ? 'text' : 'password'}
                        placeholder="Account Password or Dynamic Update Key"
                        value={domain.ddns_zone_id ?? ''}
                        onChange={(e) => setDomain({ ...domain, ddns_zone_id: e.target.value })}
                        className="pr-10 text-xs font-mono"
                      />
                      <button
                        type="button"
                        onClick={() => setShowSecret(!showSecret)}
                        className="absolute right-2.5 top-1/2 -translate-y-1/2 text-muted-foreground hover:text-foreground"
                      >
                        {showSecret ? <EyeOff className="size-3.5" /> : <Eye className="size-3.5" />}
                      </button>
                    </div>
                  </div>
                </>
              )}

              {/* FreeDNS Options */}
              {domain.ddns_provider === 'freedns' && (
                <div className="flex flex-col gap-1.5">
                  <label className="text-xs font-medium flex items-center gap-1.5">
                    <Key className="size-3.5 text-muted-foreground" />
                    FreeDNS Update Key (from direct URL)
                  </label>
                  <Input
                    placeholder="e.g. WXJ1T2... (the token string in update.php?<token>)"
                    value={domain.ddns_token ?? ''}
                    onChange={(e) => setDomain({ ...domain, ddns_token: e.target.value })}
                    className="text-xs font-mono"
                  />
                  <p className="text-[11px] text-muted-foreground">
                    Copy your random key token from the Dynamic DNS menu on freedns.afraid.org.
                  </p>
                </div>
              )}

              {/* ClouDNS Options */}
              {domain.ddns_provider === 'cloudns' && (
                <div className="flex flex-col gap-1.5">
                  <label className="text-xs font-medium flex items-center gap-1.5">
                    <Key className="size-3.5 text-muted-foreground" />
                    ClouDNS Dynamic URL or Token
                  </label>
                  <Input
                    placeholder="Dynamic URL or query token"
                    value={domain.ddns_token ?? ''}
                    onChange={(e) => setDomain({ ...domain, ddns_token: e.target.value })}
                    className="text-xs font-mono"
                  />
                </div>
              )}

              {/* Custom Webhook Options */}
              {domain.ddns_provider === 'custom' && (
                <>
                  <div className="flex flex-col gap-1.5">
                    <label className="text-xs font-medium flex items-center gap-1.5">
                      <Network className="size-3.5 text-muted-foreground" />
                      Webhook URL Endpoint
                    </label>
                    <Input
                      placeholder="https://api.example.com/dns/update?domain={domain}&ip={ip}"
                      value={domain.ddns_zone_id ?? ''}
                      onChange={(e) => setDomain({ ...domain, ddns_zone_id: e.target.value })}
                      className="text-xs font-mono"
                    />
                    <p className="text-[11px] text-muted-foreground">
                      Use <code>{'{ip}'}</code> and <code>{'{domain}'}</code> placeholders.
                    </p>
                  </div>
                  <div className="flex flex-col gap-1.5">
                    <label className="text-xs font-medium flex items-center gap-1.5">
                      <Key className="size-3.5 text-muted-foreground" />
                      Bearer Token / Key (Optional)
                    </label>
                    <Input
                      type="password"
                      placeholder="Authorization header token"
                      value={domain.ddns_token ?? ''}
                      onChange={(e) => setDomain({ ...domain, ddns_token: e.target.value })}
                      className="text-xs font-mono"
                    />
                  </div>
                </>
              )}

              {/* Native OS Public IP Detection & Sync Controls */}
              <div className="pt-2 border-t border-border/60 flex flex-col gap-2.5">
                <div className="flex items-center justify-between text-xs">
                  <span className="text-muted-foreground flex items-center gap-1.5">
                    <Cpu className="size-3.5 text-primary" />
                    Server Public IP:
                  </span>
                  <div className="flex items-center gap-2">
                    <span className="font-mono font-medium text-foreground">
                      {detectedIp ?? domain.ddns_last_ip ?? 'Not detected'}
                    </span>
                    <Button
                      type="button"
                      variant="outline"
                      size="sm"
                      disabled={detectingIp}
                      onClick={handleDetectPublicIp}
                      className="h-6 px-2 text-[11px] gap-1"
                      title="Uses native OS network commands without HTTP check"
                    >
                      <RefreshCw className={cn('size-3', detectingIp && 'animate-spin')} />
                      Detect IP
                    </Button>
                  </div>
                </div>

                {domain.ddns_last_synced_at && (
                  <div className="flex items-center justify-between text-[11px] text-muted-foreground">
                    <span>Last DNS update:</span>
                    <span>
                      {new Date(domain.ddns_last_synced_at * 1000).toLocaleString()} (IP: {domain.ddns_last_ip})
                    </span>
                  </div>
                )}

                <div className="flex items-center gap-2 pt-1">
                  <Button
                    type="button"
                    variant="secondary"
                    size="sm"
                    disabled={syncingDdns || !domain.ddns_token?.trim()}
                    onClick={() => handleSyncDdns(false)}
                    className="text-xs gap-1.5"
                  >
                    <RefreshCw className={cn('size-3.5', syncingDdns && 'animate-spin')} />
                    Sync DNS (Only if IP Changed)
                  </Button>
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    disabled={syncingDdns || !domain.ddns_token?.trim()}
                    onClick={() => handleSyncDdns(true)}
                    className="text-xs gap-1.5"
                    title="Force update even if cached IP matches"
                  >
                    <Zap className="size-3.5 text-amber-400" />
                    Force Update
                  </Button>
                </div>

                {ddnsResult && (
                  <div
                    className={cn(
                      'flex items-center gap-2 rounded-md p-2 text-xs mt-1',
                      ddnsResult.updated
                        ? 'bg-emerald-500/10 text-emerald-600 dark:text-emerald-400 border border-emerald-500/20'
                        : 'bg-muted text-muted-foreground border border-border'
                    )}
                  >
                    {ddnsResult.updated ? (
                      <CheckCircle2 className="size-3.5 shrink-0" />
                    ) : (
                      <AlertCircle className="size-3.5 shrink-0" />
                    )}
                    <span>{ddnsResult.message}</span>
                  </div>
                )}

                {/* Modem / Router NAT Loopback Notice & Solutions */}
                <div className="rounded-lg border border-amber-500/20 bg-amber-500/5 p-3.5 flex flex-col gap-2 mt-2">
                  <div className="flex items-start gap-2.5">
                    <AlertCircle className="size-4 text-amber-500 shrink-0 mt-0.5" />
                    <div className="flex flex-col gap-1 text-xs">
                      <span className="font-semibold text-amber-600 dark:text-amber-400">
                        Does opening your domain load your modem or router login page?
                      </span>
                      <p className="text-muted-foreground text-[11px] leading-relaxed">
                        When you set a domain to your Public IP and open it from inside your home network, most ISP routers don't support <strong>NAT Hairpinning (loopback)</strong> and intercept ports 80/443 to show the router's own admin page instead of forwarding to your PC.
                      </p>
                    </div>
                  </div>

                  <div className="grid grid-cols-1 md:grid-cols-3 gap-2 pt-2 border-t border-amber-500/15 text-[11px]">
                    <div className="rounded-md border border-border/60 bg-background/60 p-2.5 flex flex-col gap-1">
                      <span className="font-semibold text-foreground flex items-center gap-1 text-[11px]">
                        <Globe className="size-3 text-sky-400" /> 1. Quick Verification
                      </span>
                      <p className="text-muted-foreground text-[10.5px]">
                        Turn off Wi-Fi on your phone and open your domain over 4G/5G mobile data. External visitors will reach your site!
                      </p>
                    </div>

                    <div className="rounded-md border border-border/60 bg-background/60 p-2.5 flex flex-col gap-1">
                      <span className="font-semibold text-foreground flex items-center gap-1 text-[11px]">
                        <Zap className="size-3 text-amber-400" /> 2. Cloudflare Proxy
                      </span>
                      <p className="text-muted-foreground text-[10.5px]">
                        If using Cloudflare DDNS, turn ON <strong>Cloudflare Proxy (Orange Cloud)</strong>. Cloudflare CDN routes web traffic directly to your site, bypassing your router interface.
                      </p>
                    </div>

                    <div className="rounded-md border border-border/60 bg-background/60 p-2.5 flex flex-col gap-1">
                      <span className="font-semibold text-foreground flex items-center gap-1 text-[11px]">
                        <ShieldCheck className="size-3 text-emerald-400" /> 3. Broadcast Online (Tunnel)
                      </span>
                      <p className="text-muted-foreground text-[10.5px]">
                        Enable <strong>Broadcast Online</strong> below with Cloudflare Tunnel or Tailscale. Tunnels establish outbound connections with <em>zero router port forwarding</em>.
                      </p>
                    </div>
                  </div>
                </div>
              </div>
            </div>
          )}
        </div>
      </FormSection>

      {/* 3. SSL / HTTPS Certificate Provider */}
      <FormSection
        title="SSL & HTTPS Certificate"
        hint="Choose how certificates are provided for your primary domain and all aliases."
      >
        <div className="flex flex-col gap-4">
          <div className="grid grid-cols-3 gap-3">
            {[
              {
                id: 'local_ca',
                label: 'Local Root CA',
                desc: 'Auto-trusted local development',
                icon: <ShieldCheck className="size-4 text-emerald-400" />,
              },
              {
                id: 'lets_encrypt',
                label: "Let's Encrypt",
                desc: 'Automated public ACME certs',
                icon: <Globe className="size-4 text-sky-400" />,
              },
              {
                id: 'custom',
                label: 'Custom Files',
                desc: 'Provide your own .crt & .key',
                icon: <KeyRound className="size-4 text-indigo-400" />,
              },
            ].map((p) => {
              const active = domain.ssl_provider === p.id
              return (
                <button
                  key={p.id}
                  type="button"
                  onClick={() => setDomain({ ...domain, ssl_provider: p.id as SslProvider })}
                  className={cn(
                    'group relative flex flex-col items-start rounded-lg border p-3.5 text-left transition-all cursor-pointer',
                    active
                      ? 'border-primary bg-primary/10 shadow-sm ring-1 ring-primary'
                      : 'border-border bg-card hover:border-primary/50 hover:bg-accent/40'
                  )}
                >
                  <div className="flex w-full items-center justify-between">
                    <div
                      className={cn(
                        'flex size-7 items-center justify-center rounded-md border transition-colors',
                        active
                          ? 'border-primary/40 bg-primary/20'
                          : 'border-border/60 bg-muted/60 group-hover:border-primary/30'
                      )}
                    >
                      {p.icon}
                    </div>
                    {active && <Check className="size-3.5 text-primary" />}
                  </div>
                  <span className="mt-2.5 font-semibold text-xs text-foreground">{p.label}</span>
                  <span className="mt-1 text-[11px] leading-relaxed text-muted-foreground">{p.desc}</span>
                </button>
              )
            })}
          </div>

          {/* Let's Encrypt Details */}
          {domain.ssl_provider === 'lets_encrypt' && (
            <div className="flex flex-col gap-3 rounded-lg border border-border bg-accent/20 p-4">
              <div className="flex flex-col gap-1.5">
                <label className="text-xs font-medium">Contact Email (Optional)</label>
                <Input
                  type="email"
                  placeholder="admin@example.com (used for Let's Encrypt expiry notices)"
                  value={domain.lets_encrypt_email ?? ''}
                  onChange={(e) => setDomain({ ...domain, lets_encrypt_email: e.target.value })}
                  className="text-xs"
                />
              </div>
              <div className="flex items-center gap-2 pt-1">
                <Button
                  type="button"
                  variant="secondary"
                  size="sm"
                  disabled={issuingSsl}
                  onClick={handleIssueLetsEncrypt}
                  className="gap-1.5 text-xs"
                >
                  <Lock className="size-3.5" />
                  {issuingSsl ? 'Requesting Let’s Encrypt…' : 'Issue / Renew Certificate'}
                </Button>
              </div>
              {sslStatus && (
                <p className="text-xs text-muted-foreground font-mono bg-background p-2 rounded border border-border">
                  {sslStatus}
                </p>
              )}
            </div>
          )}

          {/* Custom SSL Files */}
          {domain.ssl_provider === 'custom' && (
            <div className="flex flex-col gap-3 rounded-lg border border-border bg-accent/20 p-4">
              <div className="flex flex-col gap-1.5">
                <label className="text-xs font-medium">Certificate File (.crt / .pem)</label>
                <div className="flex gap-2">
                  <Input
                    placeholder="C:/certs/example.crt"
                    value={domain.custom_cert_path ?? ''}
                    onChange={(e) => {
                      setDomain({ ...domain, custom_cert_path: e.target.value })
                      setSslVerifyResult(null)
                    }}
                    className="text-xs font-mono"
                  />
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    className="gap-1.5 shrink-0"
                    onClick={async () => {
                      const sel = await open({
                        filters: [{ name: 'Certificate', extensions: ['pem', 'crt', 'cer'] }],
                      })
                      if (typeof sel === 'string') {
                        setDomain({ ...domain, custom_cert_path: sel })
                        setSslVerifyResult(null)
                      }
                    }}
                  >
                    <FolderSearch className="size-3.5" />
                    Browse
                  </Button>
                </div>
              </div>

              <div className="flex flex-col gap-1.5">
                <label className="text-xs font-medium">Private Key File (.key)</label>
                <div className="flex gap-2">
                  <Input
                    placeholder="C:/certs/example.key"
                    value={domain.custom_key_path ?? ''}
                    onChange={(e) => {
                      setDomain({ ...domain, custom_key_path: e.target.value })
                      setSslVerifyResult(null)
                    }}
                    className="text-xs font-mono"
                  />
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    className="gap-1.5 shrink-0"
                    onClick={async () => {
                      const sel = await open({
                        filters: [{ name: 'Private Key', extensions: ['key', 'pem'] }],
                      })
                      if (typeof sel === 'string') {
                        setDomain({ ...domain, custom_key_path: sel })
                        setSslVerifyResult(null)
                      }
                    }}
                  >
                    <FolderSearch className="size-3.5" />
                    Browse
                  </Button>
                </div>
              </div>

              <div className="flex items-center gap-2 pt-1">
                <Button
                  type="button"
                  variant="secondary"
                  size="sm"
                  disabled={verifyingSsl || !domain.custom_cert_path || !domain.custom_key_path}
                  onClick={handleVerifyCustomSsl}
                  className="gap-1.5 text-xs"
                >
                  <FileCheck className="size-3.5" />
                  {verifyingSsl ? 'Verifying Files…' : 'Verify Certificate & Key'}
                </Button>
              </div>

              {sslVerifyResult && (
                <div
                  className={cn(
                    'flex items-center gap-2 rounded-md p-2 text-xs mt-1',
                    sslVerifyResult.valid
                      ? 'bg-emerald-500/10 text-emerald-600 dark:text-emerald-400 border border-emerald-500/20'
                      : 'bg-destructive/10 text-destructive border border-destructive/20'
                  )}
                >
                  {sslVerifyResult.valid ? (
                    <>
                      <CheckCircle2 className="size-3.5 shrink-0" />
                      <span>Certificate and private key are valid and match!</span>
                    </>
                  ) : (
                    <>
                      <AlertCircle className="size-3.5 shrink-0" />
                      <span>{sslVerifyResult.error || 'Verification failed'}</span>
                    </>
                  )}
                </div>
              )}
            </div>
          )}
        </div>
      </FormSection>

      {/* 4. Broadcast Online */}
      <FormSection
        title="Online Broadcast Tunnel"
        hint="Broadcast this site directly to the internet using encrypted Cloudflare or Localtunnel."
      >
        <div className="flex flex-col gap-3">
          <Toggle
            checked={domain.auto_broadcast ?? false}
            onChange={(auto_broadcast) => setDomain({ ...domain, auto_broadcast })}
            label="Broadcast online automatically when web stack starts"
            hint="Creates an encrypted public tunnel so remote clients and webhooks can reach this site."
          />

          {domain.auto_broadcast && (
            <div className="flex items-center gap-3 pt-1">
              <label className="text-xs font-medium text-muted-foreground flex items-center gap-1.5">
                <Wifi className="size-3.5 text-violet-400" />
                Tunnel Provider:
              </label>
              <Select
                value={domain.broadcast_provider ?? 'cloudflare'}
                onChange={(e) => setDomain({ ...domain, broadcast_provider: e.target.value })}
                className="w-60 text-xs"
              >
                <option value="cloudflare">Cloudflare Quick Tunnel</option>
                <option value="localtunnel">Localtunnel</option>
                <option value="tailscale">Tailscale Funnel</option>
              </Select>
            </div>
          )}
        </div>
      </FormSection>

      {/* Action Footer */}
      <div className="flex items-center justify-between border-t border-border pt-4">
        <div className="flex items-center gap-2">
          {saveSuccess && (
            <span className="flex items-center gap-1.5 text-xs text-emerald-600 dark:text-emerald-400 font-medium">
              <Check className="size-3.5" /> Domain settings saved and applied successfully
            </span>
          )}
        </div>
        <Button
          type="button"
          onClick={handleSave}
          disabled={saving || busy}
          className="gap-2 px-5"
        >
          {saving ? (
            <>
              <RefreshCw className="size-3.5 animate-spin" /> Saving…
            </>
          ) : (
            <>
              <Save className="size-3.5" /> Save & Apply
            </>
          )}
        </Button>
      </div>
    </div>
  )
}
