'use client'

import { useQuery } from '@tanstack/react-query'
import { useParams } from 'next/navigation'
import Link from 'next/link'
import {
  Code2, ShieldCheck, KeyRound, Globe,
  ArrowRight, CheckCircle2, Circle,
  Database, GitBranch, Zap, Activity,
  ChevronRight, Terminal, Search,
} from 'lucide-react'
import { apiFetch } from '@/lib/api'
import { useStore } from '@/state/tenantStore'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Badge } from '@/components/ui/badge'
import { cn } from '@/lib/utils'

interface Fn      { id: string; name: string; runtime: string }
interface Secret  { id: string; key: string }
interface ApiKey  { id: string; name: string; created_at: string }
interface LogEntry { id: string; level: string; message: string; timestamp: string }

function relTime(ts: string) {
  const d = Date.now() - new Date(ts).getTime()
  if (d < 60_000)      return `${Math.floor(d / 1000)}s ago`
  if (d < 3_600_000)   return `${Math.floor(d / 60_000)}m ago`
  if (d < 86_400_000)  return `${Math.floor(d / 3_600_000)}h ago`
  return `${Math.floor(d / 86_400_000)}d ago`
}

const LEVEL_DOT: Record<string, string> = {
  info:  'bg-sky-400',
  warn:  'bg-amber-400',
  error: 'bg-red-400',
  debug: 'bg-white/30',
}
const LEVEL_BADGE: Record<string, string> = {
  info:  'bg-sky-500/10 text-sky-400 border-sky-500/20',
  warn:  'bg-amber-500/10 text-amber-400 border-amber-500/20',
  error: 'bg-red-500/10 text-red-400 border-red-500/20',
  debug: 'bg-white/5 text-muted-foreground border-white/10',
}

// ─── Onboarding checklist ─────────────────────────────────────────────────────
function OnboardingChecklist({ hasFunctions, hasRoutes }: { hasFunctions: boolean; hasRoutes: boolean }) {
  const steps = [
    { done: true,         label: 'Create your project' },
    { done: hasFunctions, label: 'Deploy your first function', cmd: 'flux deploy', href: '/cli#deploy' },
    { done: hasRoutes,    label: 'Create an HTTP route',        cmd: null, href: 'routes' },
    { done: false,        label: 'Trigger a request & trace it', cmd: 'flux tail', href: '/cli#tail' },
  ]
  const completed = steps.filter(s => s.done).length

  return (
    <Card>
      <CardHeader className="pb-3">
        <div className="flex items-center justify-between">
          <CardTitle className="text-sm font-semibold">Get started</CardTitle>
          <span className="text-xs text-muted-foreground">{completed}/{steps.length}</span>
        </div>
        <div className="h-1 bg-border rounded-full mt-2 overflow-hidden">
          <div
            className="h-full bg-[#6c63ff] rounded-full transition-all"
            style={{ width: `${(completed / steps.length) * 100}%` }}
          />
        </div>
      </CardHeader>
      <CardContent className="space-y-3">
        {steps.map((s, i) => (
          <div key={i} className="flex items-start gap-3">
            {s.done
              ? <CheckCircle2 className="w-4 h-4 mt-0.5 text-emerald-400 shrink-0" />
              : <Circle className="w-4 h-4 mt-0.5 text-muted-foreground/40 shrink-0" />
            }
            <div className="flex-1 min-w-0">
              <p className={cn('text-sm leading-tight', s.done && 'line-through text-muted-foreground')}>
                {s.label}
              </p>
              {!s.done && s.cmd && (
                <code className="mt-1 inline-block text-xs bg-muted/60 border border-border px-2 py-0.5 rounded text-emerald-400 font-mono">
                  {s.cmd}
                </code>
              )}
            </div>
            {!s.done && s.href && (
              <Link href={s.href} className="shrink-0">
                <ChevronRight className="w-3.5 h-3.5 text-muted-foreground/50 hover:text-foreground transition-colors" />
              </Link>
            )}
          </div>
        ))}
      </CardContent>
    </Card>
  )
}

// ─── Execution topology ───────────────────────────────────────────────────────
function ExecutionTopology({ projectId }: { projectId: string }) {
  const nodes = [
    { icon: Globe,      label: 'Route',      sub: 'HTTP gateway',  color: '#6c63ff', bg: 'bg-[#6c63ff]/10' },
    { icon: Code2,      label: 'Function',   sub: 'Deno / Node',   color: '#3dd68c', bg: 'bg-emerald-500/10' },
    { icon: Database,   label: 'Data Engine',sub: 'Postgres',      color: '#60a5fa', bg: 'bg-blue-500/10' },
    { icon: GitBranch,  label: 'Workflow',   sub: 'Async chains',  color: '#f59e0b', bg: 'bg-amber-500/10' },
  ]

  return (
    <Card>
      <CardHeader className="pb-3">
        <CardTitle className="text-sm font-semibold">Execution flow</CardTitle>
        <p className="text-xs text-muted-foreground mt-0.5">How a request moves through your project</p>
      </CardHeader>
      <CardContent>
        <div className="flex items-center gap-0">
          {nodes.map((n, i) => (
            <div key={n.label} className="flex items-center gap-0 flex-1 min-w-0">
              <div className="flex flex-col items-center flex-1 min-w-0">
                <div className={cn('w-9 h-9 rounded-xl flex items-center justify-center mb-2', n.bg)}>
                  <n.icon className="w-4 h-4" style={{ color: n.color }} />
                </div>
                <p className="text-xs font-medium text-foreground leading-tight text-center">{n.label}</p>
                <p className="text-[10px] text-muted-foreground text-center mt-0.5 leading-tight">{n.sub}</p>
              </div>
              {i < nodes.length - 1 && (
                <ArrowRight className="w-3.5 h-3.5 text-muted-foreground/30 shrink-0 mx-1" />
              )}
            </div>
          ))}
        </div>

        <div className="mt-4 pt-3 border-t border-border flex items-center justify-between">
          <p className="text-xs text-muted-foreground">Every execution is recorded & replayable</p>
          <Link
            href={`/dashboard/projects/${projectId}/logs`}
            className="text-xs text-[#6c63ff] hover:opacity-80 flex items-center gap-1 transition-opacity"
          >
            View logs <ArrowRight className="w-3 h-3" />
          </Link>
        </div>
      </CardContent>
    </Card>
  )
}

// ─── Activity feed ────────────────────────────────────────────────────────────
function ActivityFeed({ projectId }: { projectId: string }) {
  const { data, isLoading } = useQuery({
    queryKey: ['logs-overview', projectId],
    queryFn: () => {
      const p = new URLSearchParams({ limit: '12', level: 'all' })
      return apiFetch<{ logs: LogEntry[] }>(`/logs?${p}`)
    },
    enabled: !!projectId,
    refetchInterval: 30_000,
  })

  const logs = data?.logs ?? []

  return (
    <Card className="flex flex-col">
      <CardHeader className="pb-3 flex-row items-center justify-between space-y-0">
        <div className="flex items-center gap-2">
          <Activity className="w-3.5 h-3.5 text-muted-foreground" />
          <CardTitle className="text-sm font-semibold">Recent activity</CardTitle>
          {!isLoading && logs.length > 0 && (
            <span className="relative flex h-2 w-2">
              <span className="animate-ping absolute inline-flex h-full w-full rounded-full bg-emerald-400 opacity-75" />
              <span className="relative inline-flex rounded-full h-2 w-2 bg-emerald-400" />
            </span>
          )}
        </div>
        <Link
          href={`/dashboard/projects/${projectId}/logs`}
          className="text-xs text-muted-foreground hover:text-foreground flex items-center gap-1 transition-colors"
        >
          View all <ArrowRight className="w-3 h-3" />
        </Link>
      </CardHeader>
      <CardContent className="flex-1">
        {isLoading ? (
          <div className="space-y-2.5">
            {[...Array(5)].map((_, i) => (
              <div key={i} className="flex items-center gap-3">
                <div className="w-12 h-4 rounded bg-muted/40 animate-pulse" />
                <div className="flex-1 h-4 rounded bg-muted/40 animate-pulse" />
                <div className="w-10 h-4 rounded bg-muted/40 animate-pulse" />
              </div>
            ))}
          </div>
        ) : logs.length === 0 ? (
          <div className="flex flex-col items-center justify-center py-10 gap-3">
            <div className="w-10 h-10 rounded-full bg-muted/30 flex items-center justify-center">
              <Terminal className="w-4 h-4 text-muted-foreground/50" />
            </div>
            <div className="text-center">
              <p className="text-sm text-muted-foreground">No activity yet</p>
              <p className="text-xs text-muted-foreground/60 mt-1">
                Deploy a function and trigger a request to see logs here
              </p>
            </div>
            <code className="text-xs bg-muted/40 border border-border px-3 py-1.5 rounded-lg font-mono text-emerald-400">
              flux deploy
            </code>
          </div>
        ) : (
          <div className="divide-y divide-border">
            {logs.map((log) => (
              <div key={log.id} className="flex items-start gap-3 py-2.5 first:pt-0">
                <div className="mt-1.5 shrink-0">
                  <div className={cn('w-1.5 h-1.5 rounded-full', LEVEL_DOT[log.level] ?? 'bg-white/30')} />
                </div>
                <div className="flex-1 min-w-0">
                  <p className="text-xs text-foreground/90 leading-relaxed truncate">{log.message}</p>
                </div>
                <div className="flex items-center gap-2 shrink-0">
                  <Badge
                    variant="outline"
                    className={cn('text-[10px] px-1.5 py-0 h-4 font-mono capitalize', LEVEL_BADGE[log.level])}
                  >
                    {log.level}
                  </Badge>
                  <span className="text-[10px] text-muted-foreground tabular-nums whitespace-nowrap">
                    {relTime(log.timestamp)}
                  </span>
                </div>
              </div>
            ))}
          </div>
        )}
      </CardContent>
    </Card>
  )
}

// ─── Stat card ────────────────────────────────────────────────────────────────
function StatCard({
  label, value, icon: Icon, color, bg, href, loading,
}: {
  label: string; value: number | string; icon: React.ComponentType<any>
  color: string; bg: string; href: string; loading: boolean
}) {
  return (
    <Link href={href} className="block group">
      <Card className="transition-colors group-hover:border-border/80">
        <CardHeader className="flex flex-row items-center justify-between pb-2">
          <CardTitle className="text-sm font-medium text-muted-foreground">{label}</CardTitle>
          <div className={cn('p-1.5 rounded-lg', bg)}>
            <Icon className={cn('w-3.5 h-3.5', color)} />
          </div>
        </CardHeader>
        <CardContent>
          {loading
            ? <div className="w-8 h-7 rounded bg-muted/40 animate-pulse" />
            : <p className="text-2xl font-bold tabular-nums">{value}</p>
          }
        </CardContent>
      </Card>
    </Link>
  )
}

// ─── Page ─────────────────────────────────────────────────────────────────────
export default function OverviewPage() {
  const { projectId: paramId } = useParams() as any
  const { projectId: storeId, projectName } = useStore()
  const projectId = paramId ?? storeId

  const fns = useQuery({
    queryKey: ['functions', projectId],
    queryFn: () => apiFetch<{ functions: Fn[] }>('/functions'),
    enabled: !!projectId,
  })
  const secrets = useQuery({
    queryKey: ['secrets', projectId],
    queryFn: () => apiFetch<{ secrets: Secret[] }>('/secrets'),
    enabled: !!projectId,
  })
  const routes = useQuery({
    queryKey: ['routes', projectId],
    queryFn: () => apiFetch<any[]>(`/routes?project_id=${projectId}`),
    enabled: !!projectId,
  })
  const apiKeys = useQuery({
    queryKey: ['api-keys', projectId],
    queryFn: () => apiFetch<ApiKey[]>('/api-keys'),
    enabled: !!projectId,
  })

  const functions   = fns.data?.functions ?? []
  const secretList  = secrets.data?.secrets ?? []
  const routeList   = Array.isArray(routes.data) ? routes.data : []
  const apiKeyList  = Array.isArray(apiKeys.data) ? apiKeys.data : []

  const hasFunctions = functions.length > 0
  const hasRoutes    = routeList.length > 0
  const isNew        = !hasFunctions && !hasRoutes

  const stats = [
    { label: 'Functions', value: functions.length,  icon: Code2,      color: 'text-[#6c63ff]', bg: 'bg-[#6c63ff]/10', href: `/dashboard/projects/${projectId}/functions`, loading: fns.isLoading },
    { label: 'Routes',    value: routeList.length,   icon: Globe,      color: 'text-emerald-400', bg: 'bg-emerald-500/10', href: `/dashboard/projects/${projectId}/routes`, loading: routes.isLoading },
    { label: 'Secrets',   value: secretList.length,  icon: ShieldCheck, color: 'text-blue-400', bg: 'bg-blue-500/10', href: `/dashboard/projects/${projectId}/secrets`, loading: secrets.isLoading },
    { label: 'API Keys',  value: apiKeyList.length,  icon: KeyRound,   color: 'text-amber-400', bg: 'bg-amber-500/10', href: `/dashboard/projects/${projectId}/api-keys`, loading: apiKeys.isLoading },
  ]

  return (
    <div className="p-8 max-w-5xl mx-auto space-y-6">

      {/* Header */}
      <div className="flex items-start justify-between">
        <div>
          <h1 className="text-2xl font-bold tracking-tight">{projectName ?? 'Project overview'}</h1>
          <p className="text-sm text-muted-foreground mt-1">
            Execution history · function runtime · data engine
          </p>
        </div>
        <Link
          href="/cli"
          className="flex items-center gap-2 text-xs text-muted-foreground hover:text-foreground border border-border rounded-lg px-3 py-2 transition-colors hover:bg-muted/30"
        >
          <Zap className="w-3.5 h-3.5 text-[#6c63ff]" />
          CLI reference
        </Link>
      </div>

      {/* Stat row */}
      <div className="grid grid-cols-2 lg:grid-cols-4 gap-3">
        {stats.map((s) => (
          <StatCard key={s.label} {...s} />
        ))}
      </div>

      {/* Main two-col section */}
      <div className="grid grid-cols-1 lg:grid-cols-[1fr_320px] gap-4">

        {/* Left: activity feed */}
        <ActivityFeed projectId={projectId} />

        {/* Right column */}
        <div className="space-y-4">
          <OnboardingChecklist hasFunctions={hasFunctions} hasRoutes={hasRoutes} />
          <ExecutionTopology projectId={projectId} />

          {/* Magic moment card */}
          <Card className="border-[#6c63ff]/20 bg-[#6c63ff]/[0.03]">
            <CardHeader className="pb-3">
              <div className="flex items-center gap-2">
                <Search className="w-3.5 h-3.5 text-[#a78bfa]" />
                <CardTitle className="text-sm font-semibold">Debug any failure</CardTitle>
              </div>
            </CardHeader>
            <CardContent className="space-y-3">
              <p className="text-xs text-muted-foreground leading-relaxed">
                Every request has a full execution trace. Get root-cause analysis in seconds —
                spans, inputs, outputs, errors, and file/line.
              </p>
              <div className="rounded-lg bg-black/30 border border-white/5 px-3 py-2.5 space-y-1 font-mono text-xs">
                <p className="text-muted-foreground/40 text-[10px]">CLI</p>
                <p className="text-emerald-400">flux why &lt;request-id&gt;</p>
                <p className="text-[#a78bfa]">flux trace &lt;request-id&gt;</p>
                <p className="text-sky-400">flux tail</p>
              </div>
              <Link
                href={`/dashboard/projects/${projectId}/traces`}
                className="flex items-center gap-1.5 text-xs text-[#a78bfa] hover:opacity-80 transition-opacity"
              >
                View execution timeline
                <ArrowRight className="w-3 h-3" />
              </Link>
            </CardContent>
          </Card>
        </div>
      </div>

      {/* Functions list (shown once there are some) */}
      {hasFunctions && (
        <Card>
          <CardHeader className="pb-3 flex-row items-center justify-between space-y-0">
            <CardTitle className="text-sm font-semibold">Functions</CardTitle>
            <Link
              href={`/dashboard/projects/${projectId}/functions`}
              className="text-xs text-muted-foreground hover:text-foreground flex items-center gap-1 transition-colors"
            >
              View all <ArrowRight className="w-3 h-3" />
            </Link>
          </CardHeader>
          <CardContent className="p-0">
            <div className="divide-y divide-border">
              {functions.slice(0, 6).map((fn) => (
                <Link
                  key={fn.id}
                  href={`/dashboard/projects/${projectId}/functions/${fn.id}`}
                  className="flex items-center justify-between px-6 py-3 hover:bg-muted/20 transition-colors"
                >
                  <div className="flex items-center gap-3">
                    <div className="w-7 h-7 rounded-lg bg-[#6c63ff]/10 flex items-center justify-center shrink-0">
                      <Code2 className="w-3.5 h-3.5 text-[#6c63ff]" />
                    </div>
                    <div>
                      <p className="text-sm font-medium leading-tight">{fn.name}</p>
                      <p className="text-xs text-muted-foreground mt-0.5">{fn.runtime}</p>
                    </div>
                  </div>
                  <div className="flex items-center gap-3">
                    <code className="text-xs font-mono text-muted-foreground/60 truncate max-w-[140px]">
                      {fn.id}
                    </code>
                    <ChevronRight className="w-3.5 h-3.5 text-muted-foreground/40" />
                  </div>
                </Link>
              ))}
            </div>
          </CardContent>
        </Card>
      )}
    </div>
  )
}
