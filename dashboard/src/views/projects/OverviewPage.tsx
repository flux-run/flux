'use client'

import { useQuery } from '@tanstack/react-query'
import Link from 'next/link'
import {
  Code2, ShieldCheck, KeyRound, Globe, ArrowRight, CheckCircle2, Circle,
  Activity, Terminal, Zap, ChevronRight,
} from 'lucide-react'
import { apiFetch } from '@/lib/api'
import { Badge } from '@/components/ui/badge'
import { PageHeader } from '@/components/layout/PageHeader'
import { cn } from '@/lib/utils'

interface Fn      { id: string; name: string; runtime: string; created_at: string }
interface Secret  { id: string; key: string }
interface ApiKey  { id: string; name: string; created_at: string }
interface LogEntry { id: string; level: string; message: string; timestamp: string }

function relTime(ts: string) {
  const d = Date.now() - new Date(ts).getTime()
  if (d < 60_000)     return `${Math.floor(d / 1000)}s ago`
  if (d < 3_600_000)  return `${Math.floor(d / 60_000)}m ago`
  if (d < 86_400_000) return `${Math.floor(d / 3_600_000)}h ago`
  return `${Math.floor(d / 86_400_000)}d ago`
}

const LEVEL: Record<string, { dot: string; label: string }> = {
  info:  { dot: 'bg-sky-400',   label: 'text-sky-400'   },
  warn:  { dot: 'bg-amber-400', label: 'text-amber-400' },
  error: { dot: 'bg-red-400',   label: 'text-red-400'   },
  debug: { dot: 'bg-white/20',  label: 'text-muted-foreground' },
}

function StatCard({ label, value, icon: Icon, color, href, loading }: {
  label: string; value: number; icon: React.ComponentType<any>; color: string; href: string; loading: boolean
}) {
  return (
    <Link href={href} className="group block">
      <div className="rounded-xl border bg-card p-5 hover:border-border/80 transition-colors">
        <div className="flex items-center justify-between mb-3">
          <p className="text-xs font-medium text-muted-foreground uppercase tracking-wider">{label}</p>
          <Icon className={cn('w-4 h-4', color)} />
        </div>
        {loading
          ? <div className="h-8 w-14 rounded-md bg-muted/40 animate-pulse" />
          : <p className="text-3xl font-bold tabular-nums">{value}</p>
        }
        <p className="mt-2 flex items-center gap-1 text-xs text-muted-foreground group-hover:text-foreground transition-colors">
          View all <ChevronRight className="w-3 h-3" />
        </p>
      </div>
    </Link>
  )
}

function ChecklistCard({ hasFunctions, hasRoutes }: { hasFunctions: boolean; hasRoutes: boolean }) {
  const steps = [
    { done: true,         label: 'Create your project' },
    { done: hasFunctions, label: 'Deploy your first function', cmd: 'flux deploy' },
    { done: hasRoutes,    label: 'Create an HTTP route' },
    { done: false,        label: 'Trigger a request & tail logs', cmd: 'flux tail' },
  ]
  const done = steps.filter(s => s.done).length
  return (
    <div className="rounded-xl border bg-card p-5">
      <div className="flex items-center justify-between mb-1">
        <p className="text-sm font-semibold">Getting started</p>
        <span className="text-xs text-muted-foreground tabular-nums">{done}/{steps.length}</span>
      </div>
      <div className="h-1 bg-border rounded-full mb-4 overflow-hidden">
        <div className="h-full bg-[#6c63ff] rounded-full transition-all" style={{ width: `${(done / steps.length) * 100}%` }} />
      </div>
      <div className="space-y-3">
        {steps.map((s, i) => (
          <div key={i} className="flex items-start gap-2.5">
            {s.done
              ? <CheckCircle2 className="w-4 h-4 mt-0.5 text-emerald-400 shrink-0" />
              : <Circle className="w-4 h-4 mt-0.5 text-muted-foreground/30 shrink-0" />
            }
            <div>
              <p className={cn('text-sm', s.done && 'line-through text-muted-foreground/50')}>{s.label}</p>
              {!s.done && s.cmd && (
                <code className="mt-1 inline-block text-[11px] bg-muted/60 border px-2 py-0.5 rounded font-mono text-emerald-400">{s.cmd}</code>
              )}
            </div>
          </div>
        ))}
      </div>
    </div>
  )
}

function QuickActions() {
  const p = (s: string) => `/dashboard/${s}`
  const items = [
    { icon: Code2,       label: 'New function', href: p('functions'),   color: 'text-[#a78bfa]'   },
    { icon: Globe,       label: 'Add route',    href: p('routes'),      color: 'text-emerald-400' },
    { icon: ShieldCheck, label: 'New secret',   href: p('secrets'),     color: 'text-blue-400'    },
    { icon: Terminal,    label: 'SQL query',    href: p('query'),       color: 'text-amber-400'   },
  ]
  return (
    <div className="rounded-xl border bg-card p-5">
      <p className="text-sm font-semibold mb-3">Quick actions</p>
      <div className="grid grid-cols-2 gap-2">
        {items.map((item) => (
          <Link key={item.label} href={item.href}
            className="flex items-center gap-2 p-2.5 rounded-lg border border-border/50 hover:border-border hover:bg-muted/30 transition-all"
          >
            <item.icon className={cn('w-3.5 h-3.5 shrink-0', item.color)} />
            <span className="text-xs font-medium truncate">{item.label}</span>
          </Link>
        ))}
      </div>
    </div>
  )
}

function ActivityFeed() {
  const { data, isLoading } = useQuery({
    queryKey: ['logs-overview'],
    queryFn: () => apiFetch<{ logs: LogEntry[] }>('/logs?limit=15&level=all'),
    refetchInterval: 20_000,
  })
  const logs = data?.logs ?? []
  return (
    <div className="rounded-xl border bg-card flex flex-col min-h-0">
      <div className="flex items-center justify-between px-5 py-4 border-b shrink-0">
        <div className="flex items-center gap-2">
          <Activity className="w-4 h-4 text-muted-foreground" />
          <span className="text-sm font-semibold">Live activity</span>
          {!isLoading && logs.length > 0 && (
            <span className="relative flex h-1.5 w-1.5">
              <span className="animate-ping absolute inline-flex h-full w-full rounded-full bg-emerald-400 opacity-75" />
              <span className="relative inline-flex rounded-full h-1.5 w-1.5 bg-emerald-400" />
            </span>
          )}
        </div>
        <Link href={`/dashboard/logs`} className="text-xs text-muted-foreground hover:text-foreground flex items-center gap-1 transition-colors">
          View all <ArrowRight className="w-3 h-3" />
        </Link>
      </div>
      {isLoading ? (
        <div className="p-5 space-y-3">
          {[...Array(6)].map((_, i) => <div key={i} className="flex gap-3"><div className="w-1.5 h-1.5 rounded-full bg-muted/40 mt-1.5 shrink-0" /><div className="h-3.5 flex-1 rounded bg-muted/40 animate-pulse" /><div className="h-3.5 w-16 rounded bg-muted/40 animate-pulse" /></div>)}
        </div>
      ) : logs.length === 0 ? (
        <div className="flex flex-col items-center justify-center py-16 px-6 text-center">
          <div className="w-10 h-10 rounded-xl bg-muted/40 flex items-center justify-center mb-3">
            <Terminal className="w-5 h-5 text-muted-foreground/40" />
          </div>
          <p className="text-sm font-medium mb-1">No activity yet</p>
          <p className="text-xs text-muted-foreground mb-4">Deploy a function and invoke it to see logs here.</p>
          <code className="text-xs bg-muted/60 border px-3 py-1.5 rounded-lg font-mono text-emerald-400">flux deploy</code>
        </div>
      ) : (
        <div className="divide-y divide-border/60 overflow-y-auto">
          {logs.map((log) => {
            const s = LEVEL[log.level] ?? LEVEL.info
            return (
              <div key={log.id} className="flex items-start gap-3 px-5 py-2.5 hover:bg-muted/20 transition-colors">
                <div className={cn('w-1.5 h-1.5 rounded-full mt-1.5 shrink-0', s.dot)} />
                <p className="flex-1 text-xs text-foreground/80 font-mono line-clamp-1">{log.message}</p>
                <div className="flex items-center gap-2 shrink-0">
                  <span className={cn('text-[10px] font-semibold uppercase', s.label)}>{log.level}</span>
                  <span className="text-[10px] text-muted-foreground tabular-nums">{relTime(log.timestamp)}</span>
                </div>
              </div>
            )
          })}
        </div>
      )}
    </div>
  )
}

export default function OverviewPage() {
  const p = (s: string) => `/dashboard/${s}`

  const fns     = useQuery({ queryKey: ['functions'],  queryFn: () => apiFetch<{ functions: Fn[] }>('/functions') })
  const secrets = useQuery({ queryKey: ['secrets'],    queryFn: () => apiFetch<{ secrets: Secret[] }>('/secrets') })
  const routes  = useQuery({ queryKey: ['routes'],     queryFn: () => apiFetch<any[]>('/routes') })
  const apiKeys = useQuery({ queryKey: ['api-keys'],   queryFn: () => apiFetch<ApiKey[]>('/api-keys') })

  const fnList  = fns.data?.functions ?? []
  const stats = [
    { label: 'Functions', value: fnList.length,                               icon: Code2,       color: 'text-[#a78bfa]',    href: p('functions'), loading: fns.isLoading },
    { label: 'Routes',    value: (Array.isArray(routes.data) ? routes.data : []).length, icon: Globe, color: 'text-emerald-400',href: p('routes'),    loading: routes.isLoading },
    { label: 'Secrets',   value: (secrets.data?.secrets ?? []).length,        icon: ShieldCheck, color: 'text-blue-400',     href: p('secrets'),   loading: secrets.isLoading },
    { label: 'API Keys',  value: (Array.isArray(apiKeys.data) ? apiKeys.data : []).length, icon: KeyRound, color: 'text-amber-400', href: p('api-keys'), loading: apiKeys.isLoading },
  ]

  return (
    <div className="flex flex-col h-full">
      <PageHeader
        title="Overview"
        description="Functions · routes · data · observability"
        breadcrumbs={[{ label: 'Overview' }]}
        actions={
          <Link href="/docs/cli" className="flex items-center gap-1.5 text-xs text-muted-foreground hover:text-foreground border border-border rounded-lg px-3 py-1.5 transition-colors hover:bg-muted/30">
            <Zap className="w-3.5 h-3.5 text-[#6c63ff]" />
            CLI docs
          </Link>
        }
      />
      <div className="flex-1 overflow-y-auto">
        <div className="p-6 max-w-6xl mx-auto space-y-5">
          <div className="grid grid-cols-2 lg:grid-cols-4 gap-3">
            {stats.map((s) => <StatCard key={s.label} {...s} />)}
          </div>
          <div className="grid grid-cols-1 lg:grid-cols-[1fr_300px] gap-5">
            <ActivityFeed />
            <div className="space-y-4">
              <ChecklistCard hasFunctions={fnList.length > 0} hasRoutes={(Array.isArray(routes.data) ? routes.data : []).length > 0} />
              <QuickActions />
              <div className="rounded-xl border border-[#6c63ff]/20 bg-[#6c63ff]/[0.03] p-5">
                <div className="flex items-center gap-2 mb-2">
                  <Zap className="w-3.5 h-3.5 text-[#a78bfa]" />
                  <p className="text-sm font-semibold">Debug any failure</p>
                </div>
                <p className="text-xs text-muted-foreground leading-relaxed mb-3">Every request is traced end-to-end — spans, inputs, outputs, and exact error file/line.</p>
                <div className="rounded-lg bg-black/30 border border-white/5 px-3 py-2.5 font-mono text-xs space-y-1 mb-3">
                  <p><span className="text-muted-foreground/40">$</span>{' '}<span className="text-emerald-400">flux why &lt;id&gt;</span></p>
                  <p><span className="text-muted-foreground/40">$</span>{' '}<span className="text-[#a78bfa]">flux trace &lt;id&gt;</span></p>
                  <p><span className="text-muted-foreground/40">$</span>{' '}<span className="text-sky-400">flux tail</span></p>
                </div>
                <Link href={p('traces')} className="flex items-center gap-1 text-xs text-[#a78bfa] hover:opacity-70 transition-opacity">
                  View execution traces <ArrowRight className="w-3 h-3" />
                </Link>
              </div>
            </div>
          </div>
          {fnList.length > 0 && (
            <div className="rounded-xl border bg-card overflow-hidden">
              <div className="flex items-center justify-between px-5 py-4 border-b">
                <span className="text-sm font-semibold">Functions</span>
                <Link href={p('functions')} className="text-xs text-muted-foreground hover:text-foreground flex items-center gap-1 transition-colors">View all <ArrowRight className="w-3 h-3" /></Link>
              </div>
              <div className="divide-y divide-border/60">
                {fnList.slice(0, 5).map((fn) => (
                  <Link key={fn.id} href={p(`functions/${fn.id}`)} className="flex items-center gap-3 px-5 py-3 hover:bg-muted/20 transition-colors">
                    <div className="w-1.5 h-1.5 rounded-full bg-emerald-400 shrink-0" />
                    <p className="text-sm font-medium flex-1">{fn.name}</p>
                    <Badge variant="secondary" className="font-mono text-[11px] shrink-0">{fn.runtime}</Badge>
                    <code className="text-[11px] text-muted-foreground/50 font-mono hidden sm:block">{fn.id.slice(0, 8)}…</code>
                    <ChevronRight className="w-3.5 h-3.5 text-muted-foreground/30" />
                  </Link>
                ))}
              </div>
            </div>
          )}
        </div>
      </div>
    </div>
  )
}
