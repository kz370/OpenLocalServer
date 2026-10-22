import { open } from '@tauri-apps/plugin-dialog'
import {
  AlertCircle,
  Check,
  CheckCircle2,
  Eye,
  EyeOff,
  Globe,
  Plus,
  RefreshCw,
  ShieldCheck,
  Trash2,
} from 'lucide-react'
import { useState } from 'react'

import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { FormSection, Select, Toggle } from '@/components/ui/form'
import { Input } from '@/components/ui/input'
import { type Domain, type SslProvider, runCommand } from '@/core'
import { cn } from '@/lib/utils'

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
  })

  const [newAlias, setNewAlias] = useState('')
  const [aliasError, setAliasError] = useState<string | null>(null)
  const [showToken, setShowToken] = useState(false)
  const [saving, setSaving] = useState(false)
  const [saveSuccess, setSaveSuccess] = useState(false)

  // Public IP detection state
  const [detectingIp, setDetectingIp] = useState(false)
  const [detectedIp, setDetectedIp] = useState<string | null>(null)

  // DDNS manual sync state
  const [syncingDdns, setSyncingDdns] = useState(false)
  const [ddnsResult, setDdnsResult] = useState<{ updated: boolean; message: string } | null>(null)

  // Let's Encrypt / Custom SSL issuance state
  const [issuingSsl, setIssuingSsl] = useState(false)
  const [sslStatus, setSslStatus] = useState<string | null>(null)

  function addAlias() {
    const clean = newAlias.trim().toLowerCase()
    if (!clean) return
    if (clean === domain.hostname.toLowerCase()) {
      setAliasError('Alias cannot be identical to the primary domain.')
      return
    }
    const current = domain.aliases ?? []
    if (current.map((a) => a.toLowerCase()).includes(clean)) {
      setAliasError('This alias is already added.')
      return
    }
    setAliasError(null)
    setDomain({ ...domain, aliases: [...current, clean] })
    setNewAlias('')
  }

  function removeAlias(index: number) {
    const current = domain.aliases ?? []
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
        message: 'Please select a DDNS provider and specify an API token first.',
      })
      return
    }
    setSyncingDdns(true)
    setDdnsResult(null)
    try {
      // First save settings to ensure core has latest token/config
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
      setDdnsResult({
        updated: false,
        message: `Sync failed: ${e?.problem ?? e?.message ?? e}`,
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
        setSslStatus('Certificate issued and applied successfully!')
      }
    } catch (e: any) {
      setSslStatus(`Error: ${e?.cause ?? e?.message ?? e}`)
    } finally {
      setIssuingSsl(false)
    }
  }

  async function handleSave() {
    setSaving(true)
    setSaveSuccess(false)
    try {
      await onSave(domain)
      setSaveSuccess(true)
      setTimeout(() => setSaveSuccess(false), 3000)
    } finally {
      setSaving(false)
    }
  }

  const isDdnsEnabled = Boolean(domain.ddns_provider && domain.ddns_provider !== null)

  return (
    <div className="flex flex-col gap-6">
      {/* Header */}
      <div className="border-b pb-3">
        <div className="flex items-center justify-between">
          <div>
            <h3 className="text-base font-semibold flex items-center gap-2">
              <Globe className="size-4 text-primary" />
              Domains, Dynamic DNS & SSL
            </h3>
            <p className="text-xs text-muted-foreground mt-0.5">
              Manage primary domain, domain aliases, automated Dynamic DNS updates, and HTTPS certificates.
            </p>
          </div>
          <Badge variant="outline" className="font-mono text-xs">
            {domain.hostname}
          </Badge>
        </div>
      </div>

      {/* 1. Multiple Domains & Aliases */}
      <FormSection
        title="Domain Names & Aliases"
        hint="Multiple domains can be routed to this site. Web servers will answer requests for all configured names."
      >
        <div className="flex flex-col gap-4">
          {/* Primary Domain Card */}
          <div className="flex items-center justify-between rounded-lg border border-border/80 bg-accent/30 p-3">
            <div className="flex items-center gap-3">
              <div className="rounded-md bg-primary/10 p-2 text-primary">
                <Globe className="size-4" />
              </div>
              <div>
                <div className="flex items-center gap-2">
                  <span className="font-medium text-sm">{domain.hostname}</span>
                  <Badge variant="secondary" className="text-[10px] uppercase font-bold">
                    Primary
                  </Badge>
                </div>
                <p className="text-xs text-muted-foreground">Main hostname bound to root directory.</p>
              </div>
            </div>
          </div>

          {/* Aliases List */}
          <div className="flex flex-col gap-2">
            <label className="text-xs font-semibold uppercase tracking-wider text-muted-foreground">
              Extra Domain Aliases ({(domain.aliases ?? []).length})
            </label>
            {(domain.aliases ?? []).length === 0 ? (
              <p className="text-xs text-muted-foreground italic py-1">
                No additional aliases configured. Add extra domains below.
              </p>
            ) : (
              <div className="flex flex-col gap-1.5">
                {(domain.aliases ?? []).map((alias, idx) => (
                  <div
                    key={idx}
                    className="flex items-center justify-between rounded-md border border-border bg-card px-3 py-2 text-sm"
                  >
                    <div className="flex items-center gap-2">
                      <Globe className="size-3.5 text-muted-foreground" />
                      <span className="font-mono text-xs">{alias}</span>
                    </div>
                    <Button
                      type="button"
                      variant="ghost"
                      size="sm"
                      onClick={() => removeAlias(idx)}
                      className="size-7 p-0 text-muted-foreground hover:text-destructive"
                      title="Remove alias"
                    >
                      <Trash2 className="size-3.5" />
                    </Button>
                  </div>
                ))}
              </div>
            )}

            {/* Add Alias Input */}
            <div className="flex gap-2 mt-1">
              <Input
                placeholder="e.g. api.example.com, admin.example.com"
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
                className="text-xs"
              />
              <Button type="button" variant="secondary" size="sm" onClick={addAlias} className="shrink-0 gap-1.5">
                <Plus className="size-3.5" /> Add Alias
              </Button>
            </div>
            {aliasError && <p className="text-xs text-destructive">{aliasError}</p>}
          </div>

          {/* Wildcard Subdomains */}
          <div className="pt-1">
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
        hint="Automatically updates DNS records with your provider only when your server's public IP changes."
      >
        <div className="flex flex-col gap-4">
          {/* Provider Selection Cards */}
          <div className="grid grid-cols-4 gap-2.5">
            {[
              { id: null, label: 'Disabled', desc: 'No dynamic DNS updates' },
              { id: 'cloudflare', label: 'Cloudflare', desc: 'Zones & DNS API' },
              { id: 'duckdns', label: 'DuckDNS', desc: 'Free dynamic subdomains' },
              { id: 'custom', label: 'Custom', desc: 'HTTP Webhook' },
            ].map((p) => {
              const active = domain.ddns_provider === p.id
              return (
                <button
                  key={String(p.id)}
                  type="button"
                  onClick={() =>
                    setDomain({
                      ...domain,
                      ddns_provider: p.id as any,
                    })
                  }
                  className={cn(
                    'flex flex-col items-start rounded-lg border p-3 text-left transition-colors',
                    active
                      ? 'border-primary bg-primary/5 ring-1 ring-primary'
                      : 'border-border bg-card hover:bg-accent/40'
                  )}
                >
                  <span className="font-semibold text-xs">{p.label}</span>
                  <span className="text-[11px] text-muted-foreground mt-0.5">{p.desc}</span>
                </button>
              )
            })}
          </div>

          {/* Provider Specific Settings */}
          {isDdnsEnabled && (
            <div className="flex flex-col gap-3.5 rounded-lg border border-border bg-accent/20 p-4">
              {/* Cloudflare Options */}
              {domain.ddns_provider === 'cloudflare' && (
                <>
                  <div className="flex flex-col gap-1.5">
                    <label className="text-xs font-medium">Cloudflare API Token</label>
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
                      Create a token in Cloudflare Dashboard with <code className="bg-muted px-1 py-0.5 rounded">Zone.DNS:Edit</code> template permissions.
                    </p>
                  </div>

                  <div className="flex flex-col gap-1.5">
                    <label className="text-xs font-medium">Zone ID (Optional)</label>
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

              {/* DuckDNS Options */}
              {domain.ddns_provider === 'duckdns' && (
                <div className="flex flex-col gap-1.5">
                  <label className="text-xs font-medium">DuckDNS Token</label>
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
                    DuckDNS subdomain is automatically extracted from your hostname (e.g. <code>myproject.duckdns.org</code>).
                  </p>
                </div>
              )}

              {/* Custom Webhook Options */}
              {domain.ddns_provider === 'custom' && (
                <>
                  <div className="flex flex-col gap-1.5">
                    <label className="text-xs font-medium">Webhook Endpoint URL</label>
                    <Input
                      placeholder="https://example.com/api/update-dns"
                      value={domain.ddns_zone_id ?? ''}
                      onChange={(e) => setDomain({ ...domain, ddns_zone_id: e.target.value })}
                      className="text-xs"
                    />
                  </div>

                  <div className="flex flex-col gap-1.5">
                    <label className="text-xs font-medium">Webhook Auth Token (Optional)</label>
                    <Input
                      type="password"
                      placeholder="Bearer token sent in Authorization header"
                      value={domain.ddns_token ?? ''}
                      onChange={(e) => setDomain({ ...domain, ddns_token: e.target.value })}
                      className="text-xs"
                    />
                  </div>
                </>
              )}

              {/* Server Public IP & Live Sync Bar */}
              <div className="mt-2 border-t border-border/60 pt-3">
                <div className="flex flex-col gap-2">
                  <div className="flex items-center justify-between text-xs">
                    <span className="text-muted-foreground">Server Public IP (OS Native Check):</span>
                    <div className="flex items-center gap-2">
                      <span className="font-mono font-medium">
                        {detectedIp ?? domain.ddns_last_ip ?? 'Not checked yet'}
                      </span>
                      <Button
                        type="button"
                        variant="outline"
                        size="sm"
                        disabled={detectingIp}
                        onClick={handleDetectPublicIp}
                        className="h-6 px-2 text-[11px] gap-1"
                      >
                        <RefreshCw className={cn('size-3', detectingIp && 'animate-spin')} />
                        Check IP
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
                      className="text-xs"
                      title="Force update even if cached IP matches"
                    >
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
          <div className="grid grid-cols-3 gap-2.5">
            {[
              { id: 'local_ca', label: 'Local Root CA', desc: 'Auto-trusted local development' },
              { id: 'lets_encrypt', label: "Let's Encrypt", desc: 'Automated public ACME certs' },
              { id: 'custom', label: 'Custom Files', desc: 'Provide your own .crt & .key' },
            ].map((p) => {
              const active = domain.ssl_provider === p.id
              return (
                <button
                  key={p.id}
                  type="button"
                  onClick={() => setDomain({ ...domain, ssl_provider: p.id as SslProvider })}
                  className={cn(
                    'flex flex-col items-start rounded-lg border p-3 text-left transition-colors',
                    active
                      ? 'border-primary bg-primary/5 ring-1 ring-primary'
                      : 'border-border bg-card hover:bg-accent/40'
                  )}
                >
                  <span className="font-semibold text-xs flex items-center gap-1.5">
                    <ShieldCheck className="size-3.5 text-primary" />
                    {p.label}
                  </span>
                  <span className="text-[11px] text-muted-foreground mt-0.5">{p.desc}</span>
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
                  <RefreshCw className={cn('size-3.5', issuingSsl && 'animate-spin')} />
                  Issue / Renew Certificate
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
                    onChange={(e) => setDomain({ ...domain, custom_cert_path: e.target.value })}
                    className="text-xs font-mono"
                  />
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    onClick={async () => {
                      const sel = await open({
                        filters: [{ name: 'Certificate', extensions: ['pem', 'crt', 'cer'] }],
                      })
                      if (typeof sel === 'string') setDomain({ ...domain, custom_cert_path: sel })
                    }}
                  >
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
                    onChange={(e) => setDomain({ ...domain, custom_key_path: e.target.value })}
                    className="text-xs font-mono"
                  />
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    onClick={async () => {
                      const sel = await open({
                        filters: [{ name: 'Private Key', extensions: ['key', 'pem'] }],
                      })
                      if (typeof sel === 'string') setDomain({ ...domain, custom_key_path: sel })
                    }}
                  >
                    Browse
                  </Button>
                </div>
              </div>
            </div>
          )}
        </div>
      </FormSection>

      {/* 4. Broadcast Online */}
      <FormSection
        title="Online Broadcast Tunnel"
        hint="Broadcast this site directly to the internet using Cloudflare or Localtunnel."
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
              <label className="text-xs font-medium text-muted-foreground">Tunnel Provider:</label>
              <Select
                value={domain.broadcast_provider ?? 'cloudflare'}
                onChange={(e) => setDomain({ ...domain, broadcast_provider: e.target.value })}
                className="w-56"
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
              <Check className="size-3.5" /> Save & Apply
            </>
          )}
        </Button>
      </div>
    </div>
  )
}
