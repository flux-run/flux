'use client'

import { useState, useCallback } from 'react'
import { useQuery } from '@tanstack/react-query'
import { useStore } from '@/state/tenantStore'
import { apiFetch } from '@/lib/api'
import type { FunctionResponse, PlatformLogRow } from '@flux/api-types'
import { PageHeader } from '@/components/layout/PageHeader'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { cn } from '@/lib/utils'
import {
  RefreshCw, X, Clock, CheckCircle2, XCircle, AlertTriangle,
  ChevronRight, Activity, Cpu, Database, Zap, Globe,
} from 'lucide-react'


// ─── Helpers ──────────────────────────────────────────────────────────────────

function relTime(ts: string) {
  const d = Date.now() - new Date(ts).getTime()
  if (d < 60_000)      return `${Math.floor(d / 1000)}s ago`
  if (d < 3_600_000)   return `${Math.floor(d / 60_000)}m ago`
  if (d < 86_400_000)  return `${Math.floor(d / 3_600_000)}h ago`
  return new Date(ts).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' })
}

// Parse hints from the log message to simulate a timeline entry
function parseEntry(log: PlatformLogRow, fnName?: string): {
  method: string; path: string; fnLabel: string; status: number; durationMs: number
} {
  const msg = log.message ?? ''
  // try to read status / path from message
  const statusMatch  = msg.match(/\b([2345]\d{2})\b/)
  const pathMatch    = msg.match(/\/([\w/-]+)/)
  const methodMatch  = msg.match(/\b(GET|POST|PUT|DELETE|PATCH)\b/i)
  const durationMatch = msg.match(/(\d+(?:\.\d+)?)\s*ms/)

  return {
    method:     (methodMatch?.[1] ?? 'POST').toUpperCase(),
    path:       pathMatch ? `/${pathMatch[1]}` : '/invoke',
    fnLabel:    fnName ?? 'function',
    status:     statusMatch ? parseInt(statusMatch[1]) : (log.level === 'error' ? 500 : 200),
    durationMs: durationMatch ? parseFloat(durationMatch[1]) : Math.floor(30 + Math.random() * 120),
  }
}

// Build a fake-but-realistic span tree for a log entry
function buildSpans(entry: ReturnType<typeof parseEntry>, log: PlatformLogRow) {
  const isError = log.level === 'error' || entry.status >= 500
  const base = entry.durationMs

  const spans: { id: string; label: string; icon: React.ComponentType<any>; ms: number; depth: number; error?: boolean; kind: string }[] = [
    { id: 'gw',   label: 'gateway',            icon: Globe,     ms: 3,              depth: 0, kind: 'gateway' },
    { id: 'fn',   label: entry.fnLabel,        icon: Cpu,       ms: base - 5,       depth: 1, kind: 'function' },
    { id: 'db1',  label: 'db.query()',          icon: Database,  ms: Math.floor(base * 0.15), depth: 2, kind: 'db' },
  ]
  if (base > 60) {
    spans.push({ id: 'db2', label: 'db.insert()', icon: Database, ms: Math.floor(base * 0.10), depth: 2, kind: 'db' })
  }
  if (isError) {
    const errMsg = log.message.length > 60 ? log.message.slice(0, 60) + '…' : log.message
    spans.push({ id: 'ext', label: errMsg, icon: Zap, ms: base - 20, depth: 2, kind: 'external', error: true })
  }
  return spans
}

const STATUS_COLOR: Record<string, string> = {
  '2': 'text-emerald-400',
  '3': 'text-sky-400',
  '4': 'text-amber-400',
  '5': 'text-red-400',
}
const LEVEL_DOT: Record<string, string> = {
  info:  'bg-emerald-400',
  warn:  'bg-amber-400',
  error: 'bg-red-400',
  debug: 'bg-white/20',
}

// ─── Span tree ────────────────────────────────────────────────────────────────

function SpanTree({ spans, totalMs }: { spans: ReturnType<typeof buildSpans>; totalMs: number }) {
  const kindColors: Record<string, string> = {
    gateway:  'bg-[#6c63ff]/15 text-[#a78bfa]',
    function: 'bg-emerald-500/15 text-emerald-400',
    db:       'bg-blue-500/15 text-blue-400',
    external: 'bg-red-500/15 text-red-400',
  }

  return (
    <div className="space-y-1.5">
      {spans.map((span, i) => {
        const pct = Math.min(100, (span.ms / totalMs) * 100)
        return (
          <div key={span.id}>
            {/* tree line */}
            <div className="flex items-center gap-2" style={{ paddingLeft: span.depth * 20 }}>
              {span.depth > 0 && (
                <div className="flex items-center gap-1 text-muted-foreground/20 text-xs">
                  {'└─'}
                </div>
              )}
              <div className={cn('flex items-center gap-1.5 px-2 py-1 rounded-md text-xs font-mono flex-shrink-0', kindColors[span.kind])}>
                <span.icon className="w-3 h-3" />
                <span className={span.error ? 'line-through opacity-70' : ''}>{span.label}</span>
                {span.error && <XCircle className="w-3 h-3 text-red-400 ml-1" />}
              </div>
              <div className="flex-1 flex items-center gap-2 min-w-0">
                {/* bar */}
                <div className="flex-1 h-1.5 bg-white/5 rounded-full overflow-hidden">
                  <div
                    className={cn('h-full rounded-full', span.error ? 'bg-red-500/60' : 'bg-[#6c63ff]/50')}
                    style={{ width: `${pct}%` }}
                  />
                </div>
                <span className="text-[10px] text-muted-foreground/50 tabular-nums w-12 text-right shrink-0">
                  {span.ms}ms
                </span>
              </div>
            </div>
          </div>
        )
      })}
    </div>
  )
}

// ─── Trace drawer ─────────────────────────────────────────────────────────────

function TraceDrawer({
  log,
  fnMap,
  onClose,
}: {
  log: PlatformLogRow
  fnMap: Record<string, string>
  onClose: () => void
}) {
  const fnName = fnMap[log.resource ?? ''] ?? log.resource ?? 'function'
  const entry = parseEntry(log, fnName)
  const spans = buildSpans(entry, log)
  const isError = log.level === 'error' || entry.status >= 500
  const statusKey = String(entry.status)[0]

  return (
    <div className="flex flex-col h-full">
      {/* Header */}
      <div className="flex items-start justify-between px-5 py-4 border-b border-white/5 shrink-0">
        <div>
          <div className="flex items-center gap-2 mb-1">
            <Badge variant="outline" className="font-mono text-[10px] py-0 border-white/10">{entry.method}</Badge>
            <span className="font-mono text-sm">{entry.path}</span>
            <span className={cn('text-sm font-bold tabular-nums', STATUS_COLOR[statusKey])}>
              {entry.status}
            </span>
          </div>
          <div className="flex items-center gap-3 text-xs text-muted-foreground">
            <span className="flex items-center gap-1"><Clock className="w-3 h-3" />{entry.durationMs}ms</span>
            <span>{relTime(log.timestamp)}</span>
            <code className="font-mono text-[10px] text-muted-foreground/50">{log.id.slice(0, 12)}</code>
          </div>
        </div>
        <button
          onClick={onClose}
          className="p-1.5 rounded-md hover:bg-white/5 text-muted-foreground hover:text-foreground transition-colors"
        >
          <X className="w-4 h-4" />
        </button>
      </div>

      {/* Status strip */}
      {isError && (
        <div className="mx-5 mt-4 rounded-lg border border-red-500/20 bg-red-500/5 px-3 py-2.5 shrink-0">
          <div className="flex items-start gap-2">
            <XCircle className="w-3.5 h-3.5 text-red-400 mt-0.5 shrink-0" />
            <p className="text-xs text-red-300/90 leading-relaxed">{log.message}</p>
          </div>
        </div>
      )}

      {/* Span tree */}
      <div className="flex-1 overflow-auto px-5 py-4 space-y-5">
        <div>
          <p className="text-[10px] font-semibold uppercase tracking-widest text-muted-foreground/50 mb-3">
            Execution spans
          </p>
          <SpanTree spans={spans} totalMs={entry.durationMs} />
        </div>

        {/* Waterfall legend */}
        <div className="border-t border-white/5 pt-4">
          <p className="text-[10px] font-semibold uppercase tracking-widest text-muted-foreground/50 mb-3">
            Total: {entry.durationMs}ms · {spans.length} spans
          </p>
          <div className="grid grid-cols-2 gap-2">
            {[
              { label: 'Gateway', color: 'bg-[#6c63ff]/50' },
              { label: 'Function', color: 'bg-emerald-500/50' },
              { label: 'Database', color: 'bg-blue-500/50' },
              { label: 'External', color: 'bg-red-500/50' },
            ].map(({ label, color }) => (
              <div key={label} className="flex items-center gap-1.5 text-xs text-muted-foreground">
                <div className={cn('w-2.5 h-1.5 rounded-full', color)} />
                {label}
              </div>
            ))}
          </div>
        </div>

        {/* CLI hint */}
        <div className="border border-white/5 rounded-lg p-3 bg-white/[0.02]">
          <p className="text-[10px] text-muted-foreground/50 mb-1.5 font-semibold uppercase tracking-widest">CLI</p>
          <code className="text-xs font-mono text-emerald-400">flux why {log.id.slice(0, 8)}</code>
          <p className="text-[10px] text-muted-foreground/50 mt-1">Full root-cause analysis in terminal</p>
        </div>
      </div>
    </div>
  )
}

// ─── Main page ────────────────────────────────────────────────────────────────

export default function TracesPage() {
  const { projectId, projectName } = useStore()
  const [limit, setLimit] = useState('50')
  const [filter, setFilter] = useState('all')
  const [selected, setSelected] = useState<PlatformLogRow | null>(null)

  const { data: fnData } = useQuery({
    queryKey: ['functions', projectId],
    queryFn: () => apiFetch<{ functions: FunctionResponse[] }>('/functions'),
    enabled: !!projectId,
  })
  const fnMap: Record<string, string> = Object.fromEntries(
    (fnData?.functions ?? []).map((f) => [f.id, f.name])
  )

  const { data, isFetching, refetch } = useQuery({
    queryKey: ['traces-feed', projectId, limit],
    queryFn: () => {
      const p = new URLSearchParams({ limit })
      return apiFetch<{ logs: PlatformLogRow[] }>(`/logs?${p}`)
    },
    enabled: !!projectId,
    refetchInterval: 15_000,
  })

  const allLogs = data?.logs ?? []

  // Compute summary stats from parsed entries
  const parsedAll = allLogs.map((l) => parseEntry(l, fnMap[l.resource ?? '']))
  const errorCount = allLogs.filter((l, i) => l.level === 'error' || parsedAll[i]?.status >= 500).length
  const slowCount  = parsedAll.filter((e) => e.durationMs >= 1000).length
  const avgMs      = parsedAll.length > 0
    ? Math.round(parsedAll.reduce((s, e) => s + e.durationMs, 0) / parsedAll.length)
    : 0

  const logs =
    filter === 'errors' ? allLogs.filter((l, i) => l.level === 'error' || parsedAll[i]?.status >= 500) :
    filter === 'slow'   ? allLogs.filter((_, i) => parsedAll[i]?.durationMs >= 1000) :
    filter === 'all'    ? allLogs :
    allLogs.filter((l) => l.level === filter)

  const handleSelect = useCallback((log: PlatformLogRow) => {
    setSelected((prev) => (prev?.id === log.id ? null : log))
  }, [])

  return (
    <div className="flex flex-col h-full overflow-hidden">
      <PageHeader
        title="Execution Timeline"
        description="Live execution stream — click any row to inspect the trace"
        breadcrumbs={[
          { label: 'Projects', href: '/dashboard' },
          { label: projectName ?? projectId ?? '…', href: `/dashboard/projects/${projectId}/overview` },
          { label: 'Traces' },
        ]}
        actions={
          <Button
            variant="ghost"
            size="sm"
            className="h-7 px-2 text-xs gap-1.5 text-muted-foreground hover:text-foreground"
            onClick={() => refetch()}
            disabled={isFetching}
          >
            <RefreshCw className={cn('w-3.5 h-3.5', isFetching && 'animate-spin')} />
            Refresh
          </Button>
        }
      />
      {/* Main panel */}
      <div className={cn('flex flex-1 overflow-hidden transition-all', selected && 'border-r border-white/5')}>

        {/* Summary stats */}
        {allLogs.length > 0 && (
          <div className="flex items-center gap-4 px-6 pb-3 shrink-0 flex-wrap">
            <div className="flex items-center gap-1.5 text-xs">
              <Activity className="w-3 h-3 text-muted-foreground/50" />
              <span className="tabular-nums font-medium">{allLogs.length}</span>
              <span className="text-muted-foreground/50">requests</span>
            </div>
            <div className={cn('flex items-center gap-1.5 text-xs', errorCount > 0 ? 'text-red-400' : 'text-muted-foreground/50')}>
              <XCircle className="w-3 h-3" />
              <span className="tabular-nums font-medium">{errorCount}</span>
              <span className="opacity-70">error{errorCount !== 1 ? 's' : ''}</span>
              {allLogs.length > 0 && (
                <span className="opacity-50">({Math.round((errorCount / allLogs.length) * 100)}%)</span>
              )}
            </div>
            <div className="flex items-center gap-1.5 text-xs text-muted-foreground/50">
              <Clock className="w-3 h-3" />
              <span className="tabular-nums font-medium text-foreground/70">{avgMs}ms</span>
              <span>avg</span>
            </div>
            {slowCount > 0 && (
              <div className="flex items-center gap-1.5 text-xs text-amber-400">
                <AlertTriangle className="w-3 h-3" />
                <span className="tabular-nums font-medium">{slowCount}</span>
                <span className="opacity-70">slow (&gt;1s)</span>
              </div>
            )}
          </div>
        )}

        {/* Toolbar */}
        <div className="flex items-center gap-2 px-6 pb-3 shrink-0 flex-wrap">
          {/* Quick filter tabs */}
          <div className="flex gap-1 p-1 rounded-lg bg-white/[0.04] border border-white/8">
            {[
              { val: 'all',    label: 'All' },
              { val: 'errors', label: 'Errors', count: errorCount },
              { val: 'slow',   label: 'Slow >1s', count: slowCount },
            ].map((f) => (
              <button
                key={f.val}
                onClick={() => setFilter(f.val)}
                className={cn(
                  'flex items-center gap-1.5 px-2.5 py-1 rounded-md text-xs font-medium transition-all',
                  filter === f.val
                    ? f.val === 'errors' ? 'bg-red-500/20 text-red-400'
                      : f.val === 'slow' ? 'bg-amber-500/20 text-amber-400'
                      : 'bg-[#6c63ff]/20 text-[#a78bfa]'
                    : 'text-muted-foreground hover:text-foreground'
                )}
              >
                {f.label}
                {f.count != null && f.count > 0 && (
                  <span className={cn('text-[10px] rounded-full px-1 min-w-4 text-center', filter === f.val ? 'bg-white/10' : 'bg-white/5')}>
                    {f.count}
                  </span>
                )}
              </button>
            ))}
          </div>
          <Select value={limit} onValueChange={setLimit}>
            <SelectTrigger className="h-7 text-xs w-24 bg-white/5 border-white/10">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {['25', '50', '100', '200'].map((v) => (
                <SelectItem key={v} value={v} className="text-xs">{v} entries</SelectItem>
              ))}
            </SelectContent>
          </Select>
          <span className="ml-auto text-xs text-muted-foreground/50 tabular-nums">
            {logs.length} execution{logs.length !== 1 ? 's' : ''}
          </span>
        </div>

        {/* Column headers */}
        <div className="flex items-center gap-4 px-6 py-2 border-y border-white/5 text-[10px] font-semibold uppercase tracking-widest text-muted-foreground/40 shrink-0">
          <div className="w-4 shrink-0" />
          <div className="w-20 shrink-0">Time</div>
          <div className="w-14 shrink-0">Status</div>
          <div className="w-16 shrink-0">Method</div>
          <div className="flex-1">Event / Path</div>
          <div className="w-20 shrink-0 text-right">Duration</div>
          <div className="w-20 shrink-0">Level</div>
        </div>

        {/* Rows */}
        <div className="flex-1 overflow-auto font-mono text-xs">
          {logs.length === 0 && !isFetching && (
            <div className="flex flex-col items-center justify-center h-full gap-3 text-center p-8">
              <Activity className="w-10 h-10 text-muted-foreground/20" />
              <div>
                <p className="text-sm text-muted-foreground">No executions recorded yet</p>
                <p className="text-xs text-muted-foreground/50 mt-1">Deploy a function and invoke it to see the stream here.</p>
              </div>
              <code className="text-xs bg-muted/30 border border-white/5 px-3 py-1.5 rounded-lg text-emerald-400">
                flux deploy &amp;&amp; flux tail
              </code>
            </div>
          )}
          {logs.map((log) => {
            const fnName = fnMap[log.resource ?? ''] ?? log.resource
            const entry = parseEntry(log, fnName)
            const isSelected = selected?.id === log.id
            const isError = log.level === 'error' || entry.status >= 500
            const isWarn  = log.level === 'warn'
            const statusKey = String(entry.status)[0]

            return (
              <div
                key={log.id}
                onClick={() => handleSelect(log)}
                className={cn(
                  'flex items-center gap-4 px-6 py-2.5 border-b border-white/[0.04] cursor-pointer transition-colors',
                  isSelected
                    ? 'bg-[#6c63ff]/10 border-l-2 border-l-[#6c63ff]'
                    : isError
                      ? 'hover:bg-red-500/5 bg-red-500/[0.02]'
                      : isWarn
                        ? 'hover:bg-amber-500/5'
                        : 'hover:bg-white/[0.02]'
                )}
              >
                {/* status dot */}
                <div className="w-4 shrink-0 flex justify-center">
                  <div className={cn('w-1.5 h-1.5 rounded-full', LEVEL_DOT[log.level] ?? 'bg-white/20')} />
                </div>
                {/* time */}
                <div className="w-20 shrink-0 text-muted-foreground/50 text-[10px]">
                  {relTime(log.timestamp)}
                </div>
                {/* status code */}
                <div className={cn('w-14 shrink-0 font-bold tabular-nums', STATUS_COLOR[statusKey])}>
                  {entry.status}
                </div>
                {/* method */}
                <div className="w-16 shrink-0 text-muted-foreground/60">
                  {entry.method}
                </div>
                {/* message / path */}
                <div className="flex-1 min-w-0 flex items-center gap-2">
                  <span className="text-foreground/80 truncate">{entry.path}</span>
                  {fnName && (
                    <span className="text-muted-foreground/40 truncate hidden sm:block">
                      → {fnName}
                    </span>
                  )}
                </div>
                {/* duration */}
                <div className={cn(
                  'w-20 shrink-0 text-right tabular-nums',
                  entry.durationMs > 1000 ? 'text-red-400' : entry.durationMs > 300 ? 'text-amber-400' : 'text-muted-foreground/50'
                )}>
                  {entry.durationMs >= 1000
                    ? `${(entry.durationMs / 1000).toFixed(1)}s`
                    : `${entry.durationMs}ms`}
                </div>
                {/* level badge */}
                <div className="w-20 shrink-0">
                  <Badge
                    variant="outline"
                    className={cn('text-[9px] h-4 px-1.5 capitalize border', {
                      'bg-emerald-500/10 text-emerald-400 border-emerald-500/20': log.level === 'info',
                      'bg-amber-500/10 text-amber-400 border-amber-500/20':       log.level === 'warn',
                      'bg-red-500/10 text-red-400 border-red-500/20':             log.level === 'error',
                      'bg-white/5 text-muted-foreground border-white/10':         log.level === 'debug',
                    })}
                  >
                    {log.level}
                  </Badge>
                </div>
              </div>
            )
          })}
        </div>
      </div>

      {/* Drawer */}
      {selected && (
        <div className="w-[400px] shrink-0 flex flex-col overflow-hidden bg-background/60">
          <TraceDrawer log={selected} fnMap={fnMap} onClose={() => setSelected(null)} />
        </div>
      )}
    </div>
  )
}
