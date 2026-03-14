'use client'

import { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import {
  Activity, Database, Zap, Clock, AlertCircle, CheckCircle2,
  RefreshCw, Server, TrendingUp, Wifi, WifiOff,
} from 'lucide-react'
import { apiFetch } from '@/lib/api'
import { useStore } from '@/state/tenantStore'
import { PageHeader } from '@/components/layout/PageHeader'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'
import { useEventStream, type ExecutionEvent } from '@/hooks/useEventStream'

// ─── Types ────────────────────────────────────────────────────────────────────

interface DbHealth {
  status: 'ok' | 'degraded' | 'down'
  latency_ms: number
  connections: { active: number; idle: number; max: number }
  size_bytes?: number
}

interface GatewayMetrics {
  total_requests: number
  success_rate: number    // 0..1
  p50_ms: number
  p95_ms: number
  p99_ms: number
  requests_per_minute: number
  top_routes: { path: string; method: string; count: number; avg_ms: number }[]
  error_breakdown: { status: number; count: number }[]
}

interface MonitorData {
  db: DbHealth
  gateway: GatewayMetrics
  checked_at: string
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

function fmt(n: number | undefined, unit = '') {
  if (n == null) return '—'
  if (n >= 1_000_000_000) return `${(n / 1_000_000_000).toFixed(1)}G${unit}`
  if (n >= 1_000_000)     return `${(n / 1_000_000).toFixed(1)}M${unit}`
  if (n >= 1_000)         return `${(n / 1_000).toFixed(1)}K${unit}`
  return `${n}${unit}`
}

function bytes(n?: number) {
  if (!n) return '—'
  if (n >= 1 << 30) return `${(n / (1 << 30)).toFixed(1)} GB`
  if (n >= 1 << 20) return `${(n / (1 << 20)).toFixed(1)} MB`
  return `${(n / 1024).toFixed(0)} KB`
}

function pct(n?: number) {
  if (n == null) return '—'
  return `${(n * 100).toFixed(1)}%`
}

// ─── StatusDot ────────────────────────────────────────────────────────────────

function StatusDot({ status }: { status: 'ok' | 'degraded' | 'down' }) {
  return (
    <span className={cn(
      'inline-block w-2 h-2 rounded-full',
      status === 'ok'       && 'bg-emerald-400',
      status === 'degraded' && 'bg-amber-400',
      status === 'down'     && 'bg-red-400 animate-pulse',
    )} />
  )
}

// ─── MetricCard ───────────────────────────────────────────────────────────────

function MetricCard({
  label, value, sub, icon: Icon, trend, accent,
}: {
  label: string
  value: string
  sub?: string
  icon: React.ComponentType<{ className?: string }>
  trend?: 'up' | 'down' | 'neutral'
  accent?: string
}) {
  return (
    <div className="rounded-xl border border-border bg-card p-5">
      <div className="flex items-center justify-between mb-3">
        <p className="text-xs font-medium text-muted-foreground uppercase tracking-wider">{label}</p>
        <Icon className={cn('w-4 h-4', accent ?? 'text-muted-foreground')} />
      </div>
      <p className="text-3xl font-bold tabular-nums">{value}</p>
      {sub && <p className="mt-1.5 text-xs text-muted-foreground">{sub}</p>}
    </div>
  )
}

// ─── BarChart (inline, no deps) ───────────────────────────────────────────────

function HBar({ label, value, max, color }: { label: string; value: number; max: number; color: string }) {
  const pctW = max === 0 ? 0 : Math.max(2, (value / max) * 100)
  return (
    <div className="flex items-center gap-3 text-xs">
      <span className="w-36 truncate text-muted-foreground shrink-0">{label}</span>
      <div className="flex-1 bg-muted/30 rounded-full h-1.5 overflow-hidden">
        <div className={cn('h-full rounded-full transition-all', color)} style={{ width: `${pctW}%` }} />
      </div>
      <span className="w-12 text-right tabular-nums">{fmt(value)}</span>
    </div>
  )
}

// ─── ConnectionMeter ──────────────────────────────────────────────────────────

function ConnectionMeter({ active, idle, max }: { active: number; idle: number; max: number }) {
  const usedPct = max > 0 ? ((active + idle) / max) * 100 : 0
  const color = usedPct > 80 ? 'bg-red-400' : usedPct > 60 ? 'bg-amber-400' : 'bg-emerald-400'
  return (
    <div className="space-y-2">
      <div className="flex justify-between text-xs text-muted-foreground">
        <span>DB Connections</span>
        <span>{active + idle}/{max} used</span>
      </div>
      <div className="w-full bg-muted/30 rounded-full h-2 overflow-hidden">
        <div className={cn('h-full rounded-full transition-all', color)} style={{ width: `${usedPct}%` }} />
      </div>
      <div className="flex gap-4 text-xs text-muted-foreground">
        <span><span className="text-blue-400">{active}</span> active</span>
        <span><span className="text-zinc-400">{idle}</span> idle</span>
      </div>
    </div>
  )
}

// ─── Page ─────────────────────────────────────────────────────────────────────

export default function MonitorPage() {
  const { tenantId } = useStore()
  const [refreshKey, setRefreshKey] = useState(0)

  const { data, isLoading, isError, dataUpdatedAt } = useQuery<MonitorData>({
    queryKey: ['monitor', tenantId, refreshKey],
    queryFn: () => apiFetch('/flux/api/monitor'),
    refetchInterval: 30_000,
  })

  const { events: executions, connected: execConnected, clear: clearExec } =
    useEventStream<ExecutionEvent>('executions', { maxEvents: 150 })

  const lastUpdated = dataUpdatedAt
    ? new Date(dataUpdatedAt).toLocaleTimeString()
    : null

  const db = data?.db
  const gw = data?.gateway

  return (
    <div className="space-y-6">
      <PageHeader
        title="Monitor"
        description="Real-time health and performance metrics for your Flux project."
        actions={
          <div className="flex items-center gap-3">
            {lastUpdated && (
              <p className="text-xs text-muted-foreground">Updated {lastUpdated}</p>
            )}
            <Button
              size="sm" variant="ghost"
              onClick={() => setRefreshKey(k => k + 1)}
              disabled={isLoading}
            >
              <RefreshCw className={cn('w-3.5 h-3.5 mr-1.5', isLoading && 'animate-spin')} />
              Refresh
            </Button>
          </div>
        }
      />

      {isError && (
        <div className="flex items-center gap-2 rounded-lg border border-red-900/40 bg-red-950/20 px-4 py-3 text-sm text-red-300">
          <AlertCircle className="w-4 h-4 shrink-0" />
          Failed to load metrics. Is the server running?
        </div>
      )}

      {/* ── Database health ─────────────────────────────────────────────────── */}
      <section>
        <div className="flex items-center gap-2 mb-4">
          <Database className="w-4 h-4 text-blue-400" />
          <h2 className="text-sm font-semibold">Database</h2>
          {db && <StatusDot status={db.status} />}
          {db && (
            <span className={cn(
              'text-xs',
              db.status === 'ok' ? 'text-emerald-400' : db.status === 'degraded' ? 'text-amber-400' : 'text-red-400',
            )}>
              {db.status}
            </span>
          )}
        </div>

        <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-4 mb-4">
          <MetricCard
            label="DB Latency"
            value={db ? `${db.latency_ms}ms` : '—'}
            sub={db ? (db.latency_ms < 5 ? 'Excellent' : db.latency_ms < 20 ? 'Good' : 'Slow') : undefined}
            icon={Clock}
            accent="text-blue-400"
          />
          <MetricCard
            label="Active Connections"
            value={db ? String(db.connections.active) : '—'}
            sub={db ? `${db.connections.idle} idle / ${db.connections.max} max` : undefined}
            icon={Wifi}
            accent="text-blue-400"
          />
          <MetricCard
            label="DB Size"
            value={bytes(db?.size_bytes)}
            icon={Database}
            accent="text-blue-400"
          />
          <MetricCard
            label="DB Status"
            value={db?.status === 'ok' ? 'Healthy' : db?.status ?? '—'}
            icon={db?.status === 'ok' ? CheckCircle2 : AlertCircle}
            accent={db?.status === 'ok' ? 'text-emerald-400' : 'text-red-400'}
          />
        </div>

        {db && (
          <div className="rounded-xl border border-border bg-card p-5">
            <ConnectionMeter
              active={db.connections.active}
              idle={db.connections.idle}
              max={db.connections.max}
            />
          </div>
        )}
      </section>

      {/* ── Gateway metrics ─────────────────────────────────────────────────── */}
      <section>
        <div className="flex items-center gap-2 mb-4">
          <Zap className="w-4 h-4 text-emerald-400" />
          <h2 className="text-sm font-semibold">Gateway</h2>
        </div>

        <div className="grid grid-cols-2 sm:grid-cols-3 lg:grid-cols-6 gap-4 mb-4">
          <MetricCard
            label="Total Requests"
            value={fmt(gw?.total_requests)}
            icon={Activity}
            accent="text-emerald-400"
          />
          <MetricCard
            label="Success Rate"
            value={pct(gw?.success_rate)}
            sub={gw ? `${fmt(Math.round((gw.total_requests * (1 - gw.success_rate))))} errors` : undefined}
            icon={gw && gw.success_rate >= 0.99 ? CheckCircle2 : AlertCircle}
            accent={gw && gw.success_rate >= 0.99 ? 'text-emerald-400' : 'text-amber-400'}
          />
          <MetricCard
            label="Req/min"
            value={fmt(gw?.requests_per_minute)}
            icon={TrendingUp}
            accent="text-emerald-400"
          />
          <MetricCard
            label="P50 Latency"
            value={gw ? `${gw.p50_ms}ms` : '—'}
            icon={Clock}
            accent="text-emerald-400"
          />
          <MetricCard
            label="P95 Latency"
            value={gw ? `${gw.p95_ms}ms` : '—'}
            icon={Clock}
            accent={gw && gw.p95_ms > 500 ? 'text-amber-400' : 'text-emerald-400'}
          />
          <MetricCard
            label="P99 Latency"
            value={gw ? `${gw.p99_ms}ms` : '—'}
            icon={Clock}
            accent={gw && gw.p99_ms > 1000 ? 'text-red-400' : 'text-emerald-400'}
          />
        </div>

        <div className="grid grid-cols-1 lg:grid-cols-2 gap-4">
          {/* Top routes */}
          <div className="rounded-xl border border-border bg-card p-5">
            <p className="text-xs font-medium text-muted-foreground uppercase tracking-wider mb-4">
              Top Routes by Request Count
            </p>
            {isLoading ? (
              <div className="space-y-3">
                {[1, 2, 3].map(i => <div key={i} className="h-4 bg-muted/30 rounded animate-pulse" />)}
              </div>
            ) : (gw?.top_routes ?? []).length === 0 ? (
              <p className="text-xs text-muted-foreground text-center py-4">No data yet</p>
            ) : (
              <div className="space-y-3">
                {(gw?.top_routes ?? []).map((r, i) => (
                  <div key={i}>
                    <HBar
                      label={`${r.method} ${r.path}`}
                      value={r.count}
                      max={Math.max(...(gw?.top_routes ?? []).map(x => x.count), 1)}
                      color="bg-emerald-500"
                    />
                    <p className="text-xs text-muted-foreground/60 ml-[156px] mt-0.5">avg {r.avg_ms}ms</p>
                  </div>
                ))}
              </div>
            )}
          </div>

          {/* Error breakdown */}
          <div className="rounded-xl border border-border bg-card p-5">
            <p className="text-xs font-medium text-muted-foreground uppercase tracking-wider mb-4">
              Error Status Breakdown
            </p>
            {isLoading ? (
              <div className="space-y-3">
                {[1, 2, 3].map(i => <div key={i} className="h-4 bg-muted/30 rounded animate-pulse" />)}
              </div>
            ) : (gw?.error_breakdown ?? []).length === 0 ? (
              <div className="py-8 text-center">
                <CheckCircle2 className="w-8 h-8 text-emerald-400/60 mx-auto mb-2" />
                <p className="text-xs text-muted-foreground">No errors recorded</p>
              </div>
            ) : (
              <div className="space-y-3">
                {(gw?.error_breakdown ?? []).map((e, i) => (
                  <HBar
                    key={i}
                    label={`HTTP ${e.status}`}
                    value={e.count}
                    max={Math.max(...(gw?.error_breakdown ?? []).map(x => x.count), 1)}
                    color={e.status >= 500 ? 'bg-red-500' : 'bg-amber-500'}
                  />
                ))}
              </div>
            )}
          </div>
        </div>
      </section>

      {/* ── Server info ─────────────────────────────────────────────────────── */}
      <section>
        <div className="flex items-center gap-2 mb-4">
          <Server className="w-4 h-4 text-zinc-400" />
          <h2 className="text-sm font-semibold text-muted-foreground">Server</h2>
        </div>
        <div className="rounded-xl border border-border bg-card p-5 text-xs text-muted-foreground space-y-1.5">
          <div className="flex justify-between">
            <span>Runtime</span>
            <span className="text-foreground">Rust + Tokio</span>
          </div>
          <div className="flex justify-between">
            <span>Function runtimes</span>
            <span className="text-foreground">Deno V8 · Wasmtime (8 languages)</span>
          </div>
          <div className="flex justify-between">
            <span>Data at</span>
            <span className="text-foreground font-mono">{data?.checked_at ? new Date(data.checked_at).toLocaleString() : '—'}</span>
          </div>
        </div>
      </section>

      {/* ── Live execution tail ─────────────────────────────────────────────── */}
      <section>
        <div className="flex items-center justify-between mb-4">
          <div className="flex items-center gap-2">
            {execConnected
              ? <Wifi className="w-4 h-4 text-emerald-500" />
              : <WifiOff className="w-4 h-4 text-muted-foreground" />}
            <h2 className="text-sm font-semibold text-muted-foreground">Live Requests</h2>
            <span className="text-[10px] text-muted-foreground">
              {execConnected ? 'streaming' : 'connecting…'}
            </span>
          </div>
          {executions.length > 0 && (
            <Button variant="ghost" size="sm" className="h-6 text-[10px] px-2" onClick={clearExec}>
              Clear
            </Button>
          )}
        </div>
        <div className="rounded-xl border border-border bg-card font-mono text-[11px] divide-y max-h-72 overflow-y-auto">
          {executions.length === 0 ? (
            <p className="px-4 py-6 text-center text-xs text-muted-foreground">
              No requests yet — make a database call to see live traffic here.
            </p>
          ) : (
            [...executions].reverse().map((ex, i) => (
              <div key={i} className={cn(
                'flex items-center gap-3 px-4 py-2 hover:bg-muted/10 transition-colors',
                !ex.ok && 'bg-red-500/5'
              )}>
                <span className="text-muted-foreground shrink-0 tabular-nums">
                  {new Date(ex.ts).toLocaleTimeString()}
                </span>
                <span className={cn(
                  'shrink-0 text-[10px] font-semibold px-1.5 py-0.5 rounded',
                  ex.ok ? 'bg-emerald-500/10 text-emerald-700 dark:text-emerald-400'
                        : 'bg-red-500/10 text-red-700 dark:text-red-400'
                )}>
                  {ex.status ?? '?'}
                </span>
                <span className="font-medium shrink-0">{ex.method}</span>
                <span className="text-muted-foreground truncate flex-1">{ex.path}</span>
                {ex.duration_ms != null && (
                  <span className={cn(
                    'shrink-0 tabular-nums',
                    ex.duration_ms > 500 ? 'text-amber-500' : 'text-muted-foreground'
                  )}>
                    {ex.duration_ms}ms
                  </span>
                )}
              </div>
            ))
          )}
        </div>
      </section>
    </div>
  )
}
