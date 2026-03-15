'use client'

import { useMemo } from 'react'
import { useQuery } from '@tanstack/react-query'
import { apiFetch } from '@/lib/api'
import { PageHeader } from '@/components/layout/PageHeader'
import { Badge } from '@/components/ui/badge'
import { cn } from '@/lib/utils'
import { useRouter } from 'next/navigation'
import {
  Globe, Cpu, Database, Workflow, Clock, ArrowRight,
  AlertTriangle, History, Search,
} from 'lucide-react'

// ─── Types ────────────────────────────────────────────────────────────────────

interface Fn   { id: string; name: string; runtime: string }
interface Route {
  id: string; method: string; path: string
  function_id?: string; auth_type?: string
}
interface WorkflowItem { id: string; name: string; trigger_event?: string; enabled?: boolean }
interface CronJob      { id: string; name: string; schedule?: string; enabled?: boolean }

// ─── Constants ────────────────────────────────────────────────────────────────

const METHOD_COLOR: Record<string, string> = {
  GET:    'bg-emerald-500/20 text-emerald-400 border border-emerald-500/30',
  POST:   'bg-blue-500/20 text-blue-400 border border-blue-500/30',
  PUT:    'bg-amber-500/20 text-amber-400 border border-amber-500/30',
  DELETE: 'bg-red-500/20 text-red-400 border border-red-500/30',
  PATCH:  'bg-[#6c63ff]/20 text-[#a78bfa] border border-[#6c63ff]/30',
}

// ─── Node card ────────────────────────────────────────────────────────────────

function Node({
  label,
  sub,
  icon: Icon,
  color,
  badge,
  badgeClass,
  onClick,
  dim,
}: {
  label: string
  sub?: string
  icon: React.ComponentType<{ className?: string }>
  color: string           // bg / border tints
  badge?: string
  badgeClass?: string
  onClick?: () => void
  dim?: boolean
}) {
  return (
    <div
      onClick={onClick}
      role={onClick ? 'button' : undefined}
      className={cn(
        'relative flex flex-col items-start gap-1 rounded-xl border p-3 min-w-[130px] max-w-[160px] text-left transition-all duration-150',
        color,
        onClick && 'cursor-pointer hover:scale-[1.03] hover:shadow-lg hover:shadow-black/30',
        dim && 'opacity-40',
      )}
    >
      <div className="flex items-center gap-2 w-full">
        <Icon className="w-4 h-4 shrink-0" />
        <span className="text-xs font-semibold truncate leading-tight">{label}</span>
      </div>
      {sub && <p className="text-[10px] text-current/50 leading-snug line-clamp-2 font-mono">{sub}</p>}
      {badge && (
        <Badge variant="outline" className={cn('text-[9px] h-4 px-1.5 mt-0.5', badgeClass)}>
          {badge}
        </Badge>
      )}
    </div>
  )
}

// ─── Tier label ───────────────────────────────────────────────────────────────

function TierLabel({ icon: Icon, label, color }: {
  icon: React.ComponentType<{ className?: string }>
  label: string
  color: string
}) {
  return (
    <div className={cn('flex items-center gap-2 text-xs font-semibold tracking-wide mb-3 pl-1', color)}>
      <Icon className="w-3.5 h-3.5" />
      {label}
    </div>
  )
}

// ─── SVG arrows between tiers ─────────────────────────────────────────────────

function tierArrow() {
  return (
    <div className="flex justify-center items-center py-2 text-muted-foreground/20">
      <ArrowRight className="w-5 h-5 rotate-90" />
    </div>
  )
}

// ─── Empty tier ───────────────────────────────────────────────────────────────

function EmptyTier({ message }: { message: string }) {
  return (
    <div className="flex items-center gap-2 px-3 py-2 rounded-lg border border-dashed border-white/10 text-xs text-muted-foreground/40">
      <AlertTriangle className="w-3.5 h-3.5" />
      {message}
    </div>
  )
}

// ─── Main page ────────────────────────────────────────────────────────────────

export default function TopologyPage() {
  const router = useRouter()

  const { data: fnData,       isLoading: fnLoading }  = useQuery({
    queryKey: ['functions'],
    queryFn: () => apiFetch<{ functions: Fn[] }>('/functions'),
  })
  const { data: routesData,   isLoading: rtLoading }  = useQuery({
    queryKey: ['routes'],
    queryFn: () => apiFetch<Route[]>('/routes'),
  })
  const { data: wfData,       isLoading: wfLoading }  = useQuery({
    queryKey: ['workflows'],
    queryFn: () => apiFetch<{ workflows: WorkflowItem[] }>('/db/workflows'),
  })
  const { data: cronData,     isLoading: crLoading }  = useQuery({
    queryKey: ['cron'],
    queryFn: () => apiFetch<{ cron: CronJob[] }>('/db/cron'),
  })

  const isLoading = fnLoading || rtLoading || wfLoading || crLoading

  const fns      = fnData?.functions ?? []
  const routes   = Array.isArray(routesData) ? routesData : []
  const workflows = wfData?.workflows  ?? []
  const cron     = cronData?.cron      ?? []

  // Deduplicate referenced functions
  const referencedFnIds = useMemo(
    () => new Set(routes.map((r) => r.function_id).filter(Boolean) as string[]),
    [routes]
  )
  const referencedFns = fns.filter((f) => referencedFnIds.has(f.id))
  const unreferencedFns = fns.filter((f) => !referencedFnIds.has(f.id))

  const nav = (seg: string) => router.push(`/dashboard/${seg}`)

  return (
    <div className="flex flex-col h-full overflow-auto">
      <PageHeader
        title="System Topology"
        description="Live view of your project's architecture — click any node to navigate"
        breadcrumbs={[
          { label: 'Projects', href: '/dashboard' },
          { label: 'Topology' },
        ]}
      />
      <div className="px-6 py-6 flex flex-col gap-6">

      {isLoading ? (
        <div className="grid gap-4">
          {[1, 2, 3, 4].map((i) => (
            <div key={i} className="h-32 w-full rounded-xl bg-white/5 animate-pulse" />
          ))}
        </div>
      ) : (
        <div className="flex flex-col gap-0 max-w-5xl w-full mx-auto">

          {/* ── Tier 1 · HTTP Gateway ────────────────────────────────────────── */}
          <div className="rounded-xl border border-sky-500/20 bg-sky-500/[0.03] p-4">
            <TierLabel icon={Globe} label="HTTP Gateway" color="text-sky-400" />
            {routes.length === 0 ? (
              <EmptyTier message="No routes — run flux route add to expose a function" />
            ) : (
              <div className="flex flex-wrap gap-2">
                {routes.map((r) => (
                  <Node
                    key={r.id}
                    label={r.path}
                    sub={r.function_id ? `→ ${fnData?.functions.find(f => f.id === r.function_id)?.name ?? r.function_id.slice(0, 8)}` : undefined}
                    icon={Globe}
                    badge={r.method}
                    badgeClass={METHOD_COLOR[r.method.toUpperCase()] ?? 'border-white/10'}
                    color="border-sky-500/20 bg-sky-500/[0.06] text-sky-300"
                    onClick={() => nav('routes')}
                  />
                ))}
              </div>
            )}
          </div>

          {tierArrow()}

          {/* ── Tier 2 · Functions ───────────────────────────────────────────── */}
          <div className="rounded-xl border border-[#6c63ff]/20 bg-[#6c63ff]/[0.03] p-4">
            <TierLabel icon={Cpu} label="Functions" color="text-[#a78bfa]" />
            {fns.length === 0 ? (
              <EmptyTier message="No functions — run flux deploy to get started" />
            ) : (
              <div className="flex flex-wrap gap-2">
                {referencedFns.map((f) => (
                  <Node
                    key={f.id}
                    label={f.name}
                    sub={f.runtime}
                    icon={Cpu}
                    badge="active"
                    badgeClass="bg-emerald-500/10 text-emerald-400 border-emerald-500/20"
                    color="border-[#6c63ff]/30 bg-[#6c63ff]/10 text-[#a78bfa]"
                    onClick={() => nav('functions')}
                  />
                ))}
                {unreferencedFns.map((f) => (
                  <Node
                    key={f.id}
                    label={f.name}
                    sub={f.runtime}
                    icon={Cpu}
                    badge="no route"
                    badgeClass="bg-amber-500/10 text-amber-400 border-amber-500/20"
                    color="border-[#6c63ff]/20 bg-[#6c63ff]/[0.05] text-[#a78bfa]"
                    dim
                    onClick={() => nav('functions')}
                  />
                ))}
              </div>
            )}
          </div>

          {tierArrow()}

          {/* ── Tier 3 · Data Engine ─────────────────────────────────────────── */}
          <div className="rounded-xl border border-blue-500/20 bg-blue-500/[0.03] p-4">
            <TierLabel icon={Database} label="Data Engine" color="text-blue-400" />
            <div className="flex flex-wrap gap-2">
              <Node
                label="Primary DB"
                sub="PostgreSQL"
                icon={Database}
                badge="postgres"
                badgeClass="bg-blue-500/10 text-blue-400 border-blue-500/20"
                color="border-blue-500/20 bg-blue-500/[0.08] text-blue-300"
                onClick={() => nav('data')}
              />
            </div>
          </div>

          {tierArrow()}

          {/* ── Tier 4 · Async Layer ─────────────────────────────────────────── */}
          <div className="rounded-xl border border-amber-500/20 bg-amber-500/[0.03] p-4">
            <TierLabel icon={Workflow} label="Async Layer" color="text-amber-400" />
            {workflows.length === 0 && cron.length === 0 ? (
              <EmptyTier message="No workflows or cron jobs yet" />
            ) : (
              <div className="flex flex-wrap gap-2">
                {workflows.map((w) => (
                  <Node
                    key={w.id}
                    label={w.name}
                    sub={w.trigger_event ? `on: ${w.trigger_event}` : 'workflow'}
                    icon={Workflow}
                    badge={w.enabled ? 'enabled' : 'disabled'}
                    badgeClass={
                      w.enabled
                        ? 'bg-emerald-500/10 text-emerald-400 border-emerald-500/20'
                        : 'bg-white/5 text-muted-foreground border-white/10'
                    }
                    color="border-amber-500/20 bg-amber-500/[0.08] text-amber-300"
                    onClick={() => nav('workflows')}
                  />
                ))}
                {cron.map((c) => (
                  <Node
                    key={c.id}
                    label={c.name}
                    sub={c.schedule ?? 'cron'}
                    icon={Clock}
                    badge={c.enabled ? 'enabled' : 'disabled'}
                    badgeClass={
                      c.enabled
                        ? 'bg-emerald-500/10 text-emerald-400 border-emerald-500/20'
                        : 'bg-white/5 text-muted-foreground border-white/10'
                    }
                    color="border-amber-500/20 bg-amber-500/[0.08] text-amber-300"
                    onClick={() => nav('cron')}
                  />
                ))}
              </div>
            )}
          </div>

          {tierArrow()}

          {/* ── Tier 5 · Trace History ───────────────────────────────────────── */}
          <div className="rounded-xl border border-[#6c63ff]/15 bg-[#6c63ff]/[0.02] p-4">
            <TierLabel icon={History} label="Trace History" color="text-[#a78bfa]/70" />
            <div className="flex flex-wrap gap-2">
              <Node
                label="Execution Store"
                sub="Every request recorded"
                icon={History}
                badge="queryable"
                badgeClass="bg-[#6c63ff]/10 text-[#a78bfa] border-[#6c63ff]/20"
                color="border-[#6c63ff]/20 bg-[#6c63ff]/[0.06] text-[#a78bfa]/80"
                onClick={() => nav('traces')}
              />
              <Node
                label="flux why"
                sub="root cause analysis"
                icon={Search}
                badge="CLI"
                badgeClass="bg-[#6c63ff]/10 text-[#a78bfa] border-[#6c63ff]/20"
                color="border-[#6c63ff]/15 bg-[#6c63ff]/[0.04] text-[#a78bfa]/60"
                onClick={() => nav('traces')}
              />
            </div>
            <p className="text-[10px] text-muted-foreground/30 mt-3 pl-1 leading-relaxed">
              Every execution flows into the trace store — inspect, replay, or diff any request with{' '}
              <code className="font-mono text-[#a78bfa]/60">flux why &lt;id&gt;</code>.
            </p>
          </div>

          {/* Legend / summary */}
          <div className="mt-6 flex flex-wrap gap-x-6 gap-y-2 text-xs text-muted-foreground/50 pl-1">
            <span className="flex items-center gap-1.5"><span className="w-2 h-2 rounded-full bg-sky-500/60" />HTTP Routes ({routes.length})</span>
            <span className="flex items-center gap-1.5"><span className="w-2 h-2 rounded-full bg-[#6c63ff]/60" />Functions ({fns.length})</span>
            <span className="flex items-center gap-1.5"><span className="w-2 h-2 rounded-full bg-blue-500/60" />Data Engine (2)</span>
            <span className="flex items-center gap-1.5"><span className="w-2 h-2 rounded-full bg-amber-500/60" />Async ({workflows.length + cron.length})</span>
            <span className="flex items-center gap-1.5"><span className="w-2 h-2 rounded-full bg-[#6c63ff]/40" />Trace History</span>
          </div>
        </div>
      )}
      </div>
    </div>
  )
}
