'use client'

import { useState } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { useStore } from '@/state/tenantStore'
import { apiFetch } from '@/lib/api'
import { PageHeader } from '@/components/layout/PageHeader'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Badge } from '@/components/ui/badge'
import { cn } from '@/lib/utils'
import {
  HardDrive, FileText, Lock, Globe, CheckCircle2,
  AlertTriangle, Trash2, ChevronDown, ExternalLink,
} from 'lucide-react'

// ─── Types ────────────────────────────────────────────────────────────────────

interface StorageProvider {
  id?: string
  provider: string
  bucket_name?: string
  region?: string
  endpoint_url?: string
  base_path?: string
  access_key_id?: string
  secret_access_key?: string
  is_active: boolean
  is_custom: boolean
  created_at?: string
  updated_at?: string
}

interface PresignResult { url: string; key: string; expires_in: number; bucket: string }

// ─── Provider metadata ────────────────────────────────────────────────────────

const PROVIDERS: { id: string; label: string; hint: string; needsEndpoint: boolean; defaultRegion?: string }[] = [
  { id: 'fluxbase',  label: 'Fluxbase Managed',    hint: 'Files stored in Fluxbase-managed bucket. Zero config.',                        needsEndpoint: false },
  { id: 'aws_s3',    label: 'AWS S3',               hint: 'Standard Amazon S3. Region required.',                                         needsEndpoint: false, defaultRegion: 'us-east-1' },
  { id: 'r2',        label: 'Cloudflare R2',        hint: 'S3-compatible. Endpoint: <accountid>.r2.cloudflarestorage.com.',               needsEndpoint: true },
  { id: 'do_spaces', label: 'DigitalOcean Spaces',  hint: 'S3-compatible. Endpoint: <region>.digitaloceanspaces.com.',                    needsEndpoint: true },
  { id: 'minio',     label: 'MinIO / self-hosted',  hint: 'Any S3-compatible endpoint. Specify the full URL.',                            needsEndpoint: true },
  { id: 'gcs',       label: 'Google Cloud Storage', hint: 'Via S3 interoperability. Endpoint: storage.googleapis.com.',                   needsEndpoint: true },
]

const PROVIDER_BADGE: Record<string, string> = {
  fluxbase:  'bg-[#6c63ff]/10 text-[#a78bfa] border-[#6c63ff]/20',
  aws_s3:    'bg-amber-500/10 text-amber-400 border-amber-500/20',
  r2:        'bg-orange-500/10 text-orange-400 border-orange-500/20',
  do_spaces: 'bg-blue-500/10 text-blue-400 border-blue-500/20',
  minio:     'bg-emerald-500/10 text-emerald-400 border-emerald-500/20',
  gcs:       'bg-sky-500/10 text-sky-400 border-sky-500/20',
}

// ─── Tab nav ──────────────────────────────────────────────────────────────────

function Tab({ label, active, onClick }: { label: string; active: boolean; onClick: () => void }) {
  return (
    <button
      onClick={onClick}
      className={cn(
        'px-4 py-2.5 text-sm font-medium border-b-2 transition-colors',
        active
          ? 'border-[#6c63ff] text-[#a78bfa]'
          : 'border-transparent text-muted-foreground hover:text-foreground'
      )}
    >
      {label}
    </button>
  )
}

// ─── Provider Tab ─────────────────────────────────────────────────────────────

function ProviderTab() {
  const { projectId } = useStore()
  const qc = useQueryClient()

  const { data: current, isLoading } = useQuery<StorageProvider>({
    queryKey: ['storage-provider', projectId],
    queryFn: () => apiFetch<StorageProvider>('/storage/provider'),
    enabled: !!projectId,
  })

  const [selectedProvider, setSelectedProvider] = useState<string>('')
  const [form, setForm] = useState({
    bucket_name: '', region: '', endpoint_url: '', base_path: '',
    access_key_id: '', secret_access_key: '',
  })
  const [showProviderPicker, setShowProviderPicker] = useState(false)
  const [resetConfirm, setResetConfirm] = useState(false)

  const activeProvider = selectedProvider || (current?.provider ?? 'fluxbase')
  const providerMeta = PROVIDERS.find((p) => p.id === activeProvider) ?? PROVIDERS[0]

  const upsert = useMutation({
    mutationFn: (body: Record<string, unknown>) =>
      apiFetch('/storage/provider', { method: 'PUT', body: JSON.stringify(body) }),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['storage-provider', projectId] })
      setSelectedProvider('')
    },
  })

  const reset = useMutation({
    mutationFn: () => apiFetch('/storage/provider', { method: 'DELETE' }),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['storage-provider', projectId] })
      setResetConfirm(false)
    },
  })

  function handleSave() {
    const body: Record<string, unknown> = {
      provider: activeProvider,
      bucket_name:       form.bucket_name       || undefined,
      region:            form.region            || undefined,
      endpoint_url:      form.endpoint_url      || undefined,
      base_path:         form.base_path         || undefined,
      access_key_id:     form.access_key_id     || undefined,
      secret_access_key: form.secret_access_key || undefined,
    }
    upsert.mutate(body)
  }

  const isEditing = selectedProvider !== '' && selectedProvider !== current?.provider

  if (isLoading) {
    return (
      <div className="space-y-3 p-6">
        {[1, 2, 3].map((i) => <div key={i} className="h-10 bg-white/5 rounded-lg animate-pulse" />)}
      </div>
    )
  }

  return (
    <div className="space-y-6 pt-2">
      {/* Current status card */}
      <div className={cn(
        'flex items-center gap-4 rounded-xl border p-4',
        current?.is_custom
          ? 'border-emerald-500/20 bg-emerald-500/[0.04]'
          : 'border-white/8 bg-white/[0.02]'
      )}>
        <div className={cn(
          'w-9 h-9 rounded-lg flex items-center justify-center shrink-0',
          current?.is_custom ? 'bg-emerald-500/15' : 'bg-[#6c63ff]/15'
        )}>
          <HardDrive className={cn('w-4 h-4', current?.is_custom ? 'text-emerald-400' : 'text-[#a78bfa]')} />
        </div>
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2 mb-0.5">
            <span className="text-sm font-semibold">
              {current?.is_custom ? 'Custom storage connected' : 'Fluxbase Managed Storage'}
            </span>
            <Badge variant="outline" className={cn('text-[9px] h-4 px-1.5', PROVIDER_BADGE[current?.provider ?? 'fluxbase'])}>
              {PROVIDERS.find((p) => p.id === (current?.provider ?? 'fluxbase'))?.label ?? current?.provider}
            </Badge>
          </div>
          <p className="text-xs text-muted-foreground/60">
            {current?.is_custom
              ? `Bucket: ${current.bucket_name ?? '—'}${current.region ? ` · ${current.region}` : ''}${current.base_path ? ` · prefix: ${current.base_path}` : ''}`
              : 'Files stored in Fluxbase-managed S3. Switch to a custom bucket for data ownership and cost transparency.'}
          </p>
        </div>
        {current?.is_custom && !resetConfirm && (
          <Button
            variant="ghost" size="sm"
            className="text-muted-foreground hover:text-destructive h-7 px-2 text-xs gap-1.5 shrink-0"
            onClick={() => setResetConfirm(true)}
          >
            <Trash2 className="w-3 h-3" />
            Reset
          </Button>
        )}
        {resetConfirm && (
          <div className="flex items-center gap-2 shrink-0">
            <span className="text-xs text-muted-foreground">Remove custom config?</span>
            <Button size="sm" variant="destructive" className="h-6 px-2 text-xs" onClick={() => reset.mutate()} disabled={reset.isPending}>
              {reset.isPending ? 'Removing…' : 'Yes, reset'}
            </Button>
            <Button size="sm" variant="ghost" className="h-6 px-2 text-xs" onClick={() => setResetConfirm(false)}>Cancel</Button>
          </div>
        )}
      </div>

      {/* Provider picker */}
      <div className="space-y-2">
        <Label className="text-xs font-semibold uppercase tracking-widest text-muted-foreground/50">Storage Provider</Label>
        <div className="relative">
          <button
            onClick={() => setShowProviderPicker(!showProviderPicker)}
            className="w-full flex items-center justify-between rounded-lg border border-white/10 bg-white/[0.04] px-3 py-2.5 text-sm hover:bg-white/[0.06] transition-colors"
          >
            <div className="flex items-center gap-2">
              <Badge variant="outline" className={cn('text-[9px] h-4 px-1.5', PROVIDER_BADGE[activeProvider])}>
                {providerMeta.label}
              </Badge>
              <span className="text-xs text-muted-foreground/60">{providerMeta.hint}</span>
            </div>
            <ChevronDown className={cn('w-4 h-4 text-muted-foreground/40 transition-transform', showProviderPicker && 'rotate-180')} />
          </button>
          {showProviderPicker && (
            <div className="absolute z-10 mt-1 w-full rounded-lg border border-white/10 bg-[hsl(var(--sidebar-background))] shadow-xl overflow-hidden">
              {PROVIDERS.map((p) => (
                <button
                  key={p.id}
                  onClick={() => {
                    setSelectedProvider(p.id)
                    setShowProviderPicker(false)
                    setForm({ bucket_name: '', region: p.defaultRegion ?? '', endpoint_url: '', base_path: '', access_key_id: '', secret_access_key: '' })
                  }}
                  className={cn(
                    'w-full flex items-center gap-3 px-3 py-2.5 text-left hover:bg-white/5 transition-colors',
                    activeProvider === p.id && 'bg-[#6c63ff]/10'
                  )}
                >
                  {activeProvider === p.id
                    ? <CheckCircle2 className="w-3.5 h-3.5 text-[#a78bfa] shrink-0" />
                    : <div className="w-3.5 h-3.5 rounded-full border border-white/20 shrink-0" />}
                  <div>
                    <p className="text-sm font-medium">{p.label}</p>
                    <p className="text-[11px] text-muted-foreground/60">{p.hint}</p>
                  </div>
                </button>
              ))}
            </div>
          )}
        </div>
      </div>

      {/* Config form — only for non-fluxbase providers */}
      {activeProvider !== 'fluxbase' && (
        <div className="rounded-xl border border-white/8 p-5 space-y-4">
          <p className="text-xs font-semibold uppercase tracking-widest text-muted-foreground/40 mb-4">
            {providerMeta.label} Configuration
          </p>

          <div className="grid grid-cols-2 gap-3">
            <div className="space-y-1.5">
              <Label className="text-xs">Bucket name <span className="text-destructive">*</span></Label>
              <Input placeholder="my-company-bucket" value={form.bucket_name} onChange={(e) => setForm({ ...form, bucket_name: e.target.value })} className="h-8 text-xs" />
            </div>
            <div className="space-y-1.5">
              <Label className="text-xs">Region{!providerMeta.needsEndpoint && <span className="text-destructive"> *</span>}</Label>
              <Input placeholder="us-east-1" value={form.region} onChange={(e) => setForm({ ...form, region: e.target.value })} className="h-8 text-xs" />
            </div>
          </div>

          {providerMeta.needsEndpoint && (
            <div className="space-y-1.5">
              <Label className="text-xs">Endpoint URL <span className="text-destructive">*</span></Label>
              <Input
                placeholder={
                  activeProvider === 'r2'        ? 'https://<accountid>.r2.cloudflarestorage.com' :
                  activeProvider === 'do_spaces' ? 'https://<region>.digitaloceanspaces.com' :
                  activeProvider === 'gcs'       ? 'https://storage.googleapis.com' :
                                                   'https://your-minio.example.com'
                }
                value={form.endpoint_url}
                onChange={(e) => setForm({ ...form, endpoint_url: e.target.value })}
                className="h-8 text-xs"
              />
            </div>
          )}

          <div className="space-y-1.5">
            <Label className="text-xs">Base path <span className="text-muted-foreground/40">(optional prefix inside bucket)</span></Label>
            <Input placeholder="prod/uploads" value={form.base_path} onChange={(e) => setForm({ ...form, base_path: e.target.value })} className="h-8 text-xs" />
          </div>

          <div className="border-t border-white/5 pt-4 space-y-3">
            <p className="text-[10px] font-semibold uppercase tracking-widest text-muted-foreground/30">
              Credentials <span className="normal-case font-normal text-muted-foreground/40">— AES-GCM encrypted at rest, never returned in plaintext</span>
            </p>
            <div className="grid grid-cols-2 gap-3">
              <div className="space-y-1.5">
                <Label className="text-xs">Access key ID <span className="text-destructive">*</span></Label>
                <Input
                  type="password"
                  placeholder={current?.is_custom && current.access_key_id === '***' ? '(leave blank to keep current)' : 'AKIAIOSFODNN7EXAMPLE'}
                  value={form.access_key_id}
                  onChange={(e) => setForm({ ...form, access_key_id: e.target.value })}
                  className="h-8 text-xs font-mono"
                  autoComplete="off"
                />
              </div>
              <div className="space-y-1.5">
                <Label className="text-xs">Secret access key <span className="text-destructive">*</span></Label>
                <Input
                  type="password"
                  placeholder={current?.is_custom && current.secret_access_key === '***' ? '(leave blank to keep current)' : '••••••••••••••••'}
                  value={form.secret_access_key}
                  onChange={(e) => setForm({ ...form, secret_access_key: e.target.value })}
                  className="h-8 text-xs font-mono"
                  autoComplete="off"
                />
              </div>
            </div>
          </div>

          {upsert.isError && (
            <div className="flex items-center gap-2 text-xs text-destructive">
              <AlertTriangle className="w-3.5 h-3.5 shrink-0" />
              {(upsert.error as Error)?.message ?? 'Save failed. Check your credentials and bucket name.'}
            </div>
          )}
          {upsert.isSuccess && !isEditing && (
            <div className="flex items-center gap-2 text-xs text-emerald-400">
              <CheckCircle2 className="w-3.5 h-3.5 shrink-0" />
              Configuration saved successfully.
            </div>
          )}

          <Button className="w-full" disabled={upsert.isPending || !form.bucket_name} onClick={handleSave}>
            {upsert.isPending ? 'Saving…' : current?.is_custom ? 'Update configuration' : 'Connect custom bucket'}
          </Button>
        </div>
      )}

      {/* Fluxbase — save when switching back from custom */}
      {activeProvider === 'fluxbase' && isEditing && (
        <Button className="w-full" onClick={handleSave} disabled={upsert.isPending}>
          {upsert.isPending ? 'Saving…' : 'Switch to Fluxbase Managed'}
        </Button>
      )}

      {/* Architecture note */}
      <div className="rounded-xl border border-white/5 bg-white/[0.02] p-4 space-y-2">
        <p className="text-[10px] font-semibold uppercase tracking-widest text-muted-foreground/30">Object key structure</p>
        <p className="text-xs text-muted-foreground/60 leading-relaxed">
          Files stream directly to your bucket via a pre-signed URL — they never pass through Fluxbase servers.
        </p>
        <pre className="text-[11px] font-mono bg-black/20 border border-white/5 rounded-lg px-3 py-2 text-muted-foreground/70">
          {form.base_path || '<base_path>'}/<span className="text-sky-400">tenant</span>/<span className="text-[#a78bfa]">project</span>/<span className="text-emerald-400">table</span>/<span className="text-amber-400">row_id</span>/<span className="text-blue-400">column</span>/<span className="text-muted-foreground/40">uuid</span>
        </pre>
        <div className="flex items-center gap-4 pt-1">
          <div className="flex items-center gap-1.5 text-xs text-muted-foreground/50">
            <Globe className="w-3 h-3 text-emerald-400" />
            <span><strong className="text-foreground/70">Public</strong> — direct download URL</span>
          </div>
          <div className="flex items-center gap-1.5 text-xs text-muted-foreground/50">
            <Lock className="w-3 h-3 text-amber-400" />
            <span><strong className="text-foreground/70">Private</strong> — signed URL, 15 min TTL</span>
          </div>
        </div>
      </div>
    </div>
  )
}

// ─── Upload tester tab ────────────────────────────────────────────────────────

function TesterTab() {
  const [table, setTable]   = useState('')
  const [column, setColumn] = useState('')
  const [rowId, setRowId]   = useState('')
  const [kind, setKind]     = useState<'upload' | 'download'>('upload')
  const [result, setResult] = useState<PresignResult | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  async function generate() {
    if (!table || !column || !rowId) return
    setLoading(true); setError(null); setResult(null)
    try {
      const r = await apiFetch<PresignResult>('/storage/presign', {
        method: 'POST',
        body: JSON.stringify({ table, column, row_id: rowId, kind }),
      })
      setResult(r)
    } catch (e) {
      setError((e as Error).message)
    } finally {
      setLoading(false)
    }
  }

  return (
    <div className="space-y-5 pt-2">
      <div className="grid grid-cols-2 gap-3">
        <div className="space-y-1.5">
          <Label className="text-xs">Table</Label>
          <Input placeholder="users" value={table} onChange={(e) => setTable(e.target.value)} className="h-8 text-xs" />
        </div>
        <div className="space-y-1.5">
          <Label className="text-xs">File column</Label>
          <Input placeholder="avatar" value={column} onChange={(e) => setColumn(e.target.value)} className="h-8 text-xs" />
        </div>
      </div>

      <div className="space-y-1.5">
        <Label className="text-xs">Row ID</Label>
        <Input placeholder="uuid of the row" value={rowId} onChange={(e) => setRowId(e.target.value)} className="h-8 text-xs" />
      </div>

      <div className="flex gap-1 p-1 rounded-lg bg-white/[0.04] border border-white/8 w-fit">
        {(['upload', 'download'] as const).map((k) => (
          <button
            key={k}
            onClick={() => setKind(k)}
            className={cn(
              'px-3 py-1 rounded-md text-xs font-medium transition-all capitalize',
              kind === k ? 'bg-[#6c63ff]/20 text-[#a78bfa]' : 'text-muted-foreground hover:text-foreground'
            )}
          >
            {k}
          </button>
        ))}
      </div>

      <Button className="w-full" disabled={!table || !column || !rowId || loading} onClick={generate}>
        {loading ? 'Generating…' : `Generate ${kind} URL`}
      </Button>

      {error && (
        <div className="flex items-center gap-2 text-xs text-destructive">
          <AlertTriangle className="w-3.5 h-3.5" />{error}
        </div>
      )}

      {result && (
        <div className="rounded-xl border border-white/8 bg-white/[0.02] p-4 space-y-3">
          <div>
            <p className="text-[10px] text-muted-foreground/40 uppercase tracking-widest mb-1.5">
              {kind === 'upload' ? 'PUT to this URL' : 'GET from this URL'}
              <span className="ml-2 normal-case font-normal"> · expires in {result.expires_in}s</span>
            </p>
            <p className="font-mono text-[11px] break-all text-muted-foreground/70 bg-black/20 border border-white/5 rounded-lg px-3 py-2">{result.url}</p>
          </div>
          <div>
            <p className="text-[10px] text-muted-foreground/40 uppercase tracking-widest mb-1.5">Object key (stored in row)</p>
            <p className="font-mono text-[11px] text-[#a78bfa] bg-black/20 border border-white/5 rounded-lg px-3 py-2">{result.key}</p>
          </div>
          <div className="flex items-center justify-between pt-1 text-[10px] text-muted-foreground/40">
            <span>Bucket: {result.bucket}</span>
            <a href={result.url} target="_blank" rel="noopener noreferrer" className="flex items-center gap-1 hover:text-foreground transition-colors">
              Open <ExternalLink className="w-3 h-3" />
            </a>
          </div>
        </div>
      )}

      <div className="rounded-xl border border-white/5 bg-white/[0.02] p-4">
        <p className="text-[10px] font-semibold uppercase tracking-widest text-muted-foreground/30 mb-3">SDK Usage</p>
        <pre className="text-[11px] font-mono text-muted-foreground/70 leading-relaxed overflow-x-auto">
{`// Get a pre-signed upload URL
const { url, key } = await fb.storage.presign({
  table: '${table || 'users'}',
  column: '${column || 'avatar'}',
  rowId: '${rowId || '<uuid>'}',
  kind: 'upload',
})

// Stream file directly to bucket (never via Fluxbase API)
await fetch(url, {
  method: 'PUT',
  body: file,
  headers: { 'Content-Type': file.type },
})

// Persist the key on the row
await fb.db.${table || 'users'}.update({ id: rowId, ${column || 'avatar'}: key })`}
        </pre>
      </div>
    </div>
  )
}

// ─── Main page ────────────────────────────────────────────────────────────────

export default function StoragePage() {
  const { projectId, projectName } = useStore()
  const [tab, setTab] = useState<'provider' | 'tester'>('provider')

  return (
    <div className="flex flex-col h-full overflow-auto">
      <PageHeader
        title="Storage"
        description="Connect your own S3-compatible bucket or use Fluxbase-managed storage"
        breadcrumbs={[
          { label: 'Projects', href: '/dashboard' },
          { label: projectName ?? projectId ?? '…', href: `/dashboard/projects/${projectId}/overview` },
          { label: 'Storage' },
        ]}
      />
      <div className="px-6 pt-4 pb-0 shrink-0">
        <div className="flex gap-0 border-b border-white/8">
          <Tab label="Storage Provider" active={tab === 'provider'} onClick={() => setTab('provider')} />
          <Tab label="URL Tester"       active={tab === 'tester'}   onClick={() => setTab('tester')} />
        </div>
      </div>

      <div className="flex-1 overflow-auto px-6 py-5 max-w-2xl">
        {tab === 'provider' && <ProviderTab />}
        {tab === 'tester'   && <TesterTab />}
      </div>
    </div>
  )
}
