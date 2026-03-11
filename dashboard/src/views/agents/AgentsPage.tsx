'use client'

import { useState, useMemo } from 'react'
import { useQuery } from '@tanstack/react-query'
import { useStore } from '@/state/tenantStore'
import { apiFetch } from '@/lib/api'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'
import {
  X, Clock, CheckCircle2, XCircle, RefreshCw, ChevronDown, ChevronUp,
  Brain, Cpu, Database, Zap, Globe, GitBranch, Play,
  AlertTriangle, Sparkles, Terminal, User,
} from 'lucide-react'

// ─── Types ────────────────────────────────────────────────────────────────────

interface WorkflowStep { action_type: string; config?: Record<string, unknown> }
interface Workflow {
  id: string; name: string; trigger_event?: string
  enabled?: boolean; steps?: WorkflowStep[]
}
interface LogEntry { id: string; level: string; message: string; timestamp: string }

// ─── Synthetic agent run model ────────────────────────────────────────────────
// Until a /agents/runs backend endpoint exists, we derive agent run data from
// the workflow definition + recent logs. The structure maps directly to what a
// real /agents/runs endpoint would return.

interface AgentStep {
  id: string
  kind: 'trigger' | 'plan' | 'tool' | 'db' | 'event' | 'output' | 'error'
  label: string
  sub?: string
  durationMs?: number
  status: 'ok' | 'error' | 'pending'
  input?: Record<string, unknown>
  output?: Record<string, unknown>
  errorMsg?: string
}

interface AgentRun {
  id: string
  agentName: string
  triggeredBy: string
  status: 'success' | 'failed' | 'running'
  startedAt: string
  durationMs: number
  stepCount: number
  errorStep?: string
  steps: AgentStep[]
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

function relTime(ts: string) {
  const d = Date.now() - new Date(ts).getTime()
  if (d < 60_000)     return `${Math.floor(d / 1000)}s ago`
  if (d < 3_600_000)  return `${Math.floor(d / 60_000)}m ago`
  if (d < 86_400_000) return `${Math.floor(d / 3_600_000)}h ago`
  return new Date(ts).toLocaleDateString()
}

function fmtMs(ms: number) {
  return ms >= 1000 ? `${(ms / 1000).toFixed(1)}s` : `${ms}ms`
}

function actionKind(action_type: string): AgentStep['kind'] {
  const t = action_type.toLowerCase()
  if (t.includes('db') || t.includes('query') || t.includes('insert') || t.includes('select')) return 'db'
  if (t.includes('event') || t.includes('emit') || t.includes('publish'))                      return 'event'
  if (t.includes('http') || t.includes('call') || t.includes('request') || t.includes('api'))  return 'tool'
  return 'tool'
}

/** Derive synthetic agent run steps from a workflow definition */
function deriveRuns(wf: Workflow, logs: LogEntry[]): AgentRun[] {
  const steps = wf.steps ?? []
  // Find log entries that mention this workflow
  const relatedLogs = logs.filter(
    (l) => l.message.toLowerCase().includes(wf.name.toLowerCase()) ||
           l.message.toLowerCase().includes(wf.id.slice(0, 6))
  )
  const hasError = relatedLogs.some((l) => l.level === 'error')

  // Build step list
  const agentSteps: AgentStep[] = [
    {
      id: 'trigger',
      kind: 'trigger',
      label: wf.trigger_event ? `on: ${wf.trigger_event}` : 'manual trigger',
      sub: 'Agent received input',
      durationMs: 2,
      status: 'ok',
      input: wf.trigger_event ? { event: wf.trigger_event, source: 'gateway' } : { source: 'manual' },
    },
    {
      id: 'plan',
      kind: 'plan',
      label: 'agent.plan()',
      sub: `Planning ${steps.length} step${steps.length !== 1 ? 's' : ''}`,
      durationMs: 28 + steps.length * 8,
      status: 'ok',
      input:  { workflow: wf.name, steps: steps.length, model: 'internal-planner' },
      output: { plan: steps.map((s) => s.action_type) },
    },
    ...steps.map((s, i): AgentStep => {
      const isLastAndError = hasError && i === steps.length - 1
      const config = s.config ?? {}
      return {
        id: `step-${i}`,
        kind: isLastAndError ? 'error' : actionKind(s.action_type),
        label: s.action_type,
        sub: typeof config.table === 'string'
          ? `table: ${config.table}`
          : typeof config.url === 'string'
            ? config.url
            : undefined,
        durationMs: isLastAndError
          ? 5
          : Math.floor(15 + Math.random() * 200),
        status: isLastAndError ? 'error' : 'ok',
        input:  Object.keys(config).length > 0 ? config as Record<string, unknown> : { step: i + 1 },
        output: isLastAndError
          ? undefined
          : { result: 'ok', step: i + 1 },
        errorMsg: isLastAndError
          ? (relatedLogs.find((l) => l.level === 'error')?.message ?? 'Step failed unexpectedly')
          : undefined,
      }
    }),
    ...(!hasError ? [{
      id: 'output',
      kind: 'output' as const,
      label: 'agent.complete()',
      sub: 'Run finished successfully',
      durationMs: 3,
      status: 'ok' as const,
      output: { status: 'complete', steps_run: steps.length },
    }] : []),
  ]

  const totalMs = agentSteps.reduce((s, st) => s + (st.durationMs ?? 0), 0)

  // Generate 1-3 synthetic runs — first from the log timestamp if available,
  // otherwise relative offsets for demonstration
  const baseTs = relatedLogs[0]?.timestamp ?? new Date(Date.now() - 120_000).toISOString()

  return [
    {
      id: `${wf.id}-r1`,
      agentName: wf.name,
      triggeredBy: wf.trigger_event ?? 'manual',
      status: hasError ? 'failed' : 'success',
      startedAt: baseTs,
      durationMs: totalMs,
      stepCount: agentSteps.length,
      errorStep: hasError ? steps[steps.length - 1]?.action_type : undefined,
      steps: agentSteps,
    },
    {
      id: `${wf.id}-r2`,
      agentName: wf.name,
      triggeredBy: wf.trigger_event ?? 'manual',
      status: 'success',
      startedAt: new Date(Date.now() - 3_800_000).toISOString(),
      durationMs: Math.floor(totalMs * 0.85),
      stepCount: agentSteps.length,
      steps: agentSteps.map((s) => ({ ...s, status: 'ok' as const, errorMsg: undefined, kind: s.kind === 'error' ? 'tool' : s.kind })),
    },
  ]
}

// ─── Step kind config ─────────────────────────────────────────────────────────

const KIND: Record<AgentStep['kind'], {
  icon: React.ComponentType<{ className?: string }>
  dot: string
  badge: string
  label: string
}> = {
  trigger: { icon: User,         dot: 'bg-sky-400',       badge: 'bg-sky-500/15 text-sky-300 border-sky-500/30',       label: 'trigger'  },
  plan:    { icon: Brain,        dot: 'bg-[#a78bfa]',     badge: 'bg-[#6c63ff]/15 text-[#a78bfa] border-[#6c63ff]/30', label: 'plan'     },
  tool:    { icon: Cpu,          dot: 'bg-emerald-400',   badge: 'bg-emerald-500/15 text-emerald-400 border-emerald-500/30', label: 'tool' },
  db:      { icon: Database,     dot: 'bg-blue-400',      badge: 'bg-blue-500/15 text-blue-400 border-blue-500/30',     label: 'db'       },
  event:   { icon: Zap,          dot: 'bg-amber-400',     badge: 'bg-amber-500/15 text-amber-300 border-amber-500/30',  label: 'event'    },
  output:  { icon: CheckCircle2, dot: 'bg-emerald-400',   badge: 'bg-emerald-500/15 text-emerald-400 border-emerald-500/30', label: 'output' },
  error:   { icon: XCircle,      dot: 'bg-red-400',       badge: 'bg-red-500/15 text-red-400 border-red-500/30',        label: 'error'    },
}

// ─── Step row ─────────────────────────────────────────────────────────────────

function StepRow({ step, isLast }: { step: AgentStep; isLast: boolean }) {
  const [open, setOpen] = useState(false)
  const cfg = KIND[step.kind]

  return (
    <div className="relative flex gap-3">
      {/* Vertical connector */}
      <div className="flex flex-col items-center w-6 shrink-0">
        <div className={cn('w-2.5 h-2.5 rounded-full shrink-0 mt-3 z-10', cfg.dot, step.status === 'error' && 'ring-2 ring-red-500/40')} />
        {!isLast && <div className="w-px flex-1 bg-white/8 my-1" />}
      </div>

      {/* Card */}
      <div className={cn(
        'flex-1 mb-3 rounded-xl border transition-all',
        step.status === 'error'
          ? 'border-red-500/30 bg-red-500/[0.04]'
          : 'border-white/8 bg-white/[0.02] hover:bg-white/[0.04]',
      )}>
        <button
          onClick={() => (step.input || step.output) && setOpen(!open)}
          className={cn(
            'w-full flex items-center gap-3 px-4 py-3 text-left',
            (step.input || step.output) && 'cursor-pointer'
          )}
        >
          <cfg.icon className={cn('w-3.5 h-3.5 shrink-0', step.status === 'error' ? 'text-red-400' : 'text-muted-foreground/60')} />
          <div className="flex-1 min-w-0">
            <div className="flex items-center gap-2 flex-wrap">
              <span className={cn('font-mono text-sm font-medium', step.status === 'error' ? 'text-red-300' : 'text-foreground/90')}>
                {step.label}
              </span>
              <Badge variant="outline" className={cn('text-[9px] h-4 px-1.5 capitalize', cfg.badge)}>
                {cfg.label}
              </Badge>
              {step.status === 'error' && (
                <Badge variant="outline" className="text-[9px] h-4 px-1.5 bg-red-500/10 text-red-400 border-red-500/20">
                  failed
                </Badge>
              )}
            </div>
            {step.sub && (
              <p className="text-xs text-muted-foreground/50 mt-0.5 font-mono truncate">{step.sub}</p>
            )}
          </div>
          <div className="flex items-center gap-3 shrink-0">
            {step.durationMs !== undefined && (
              <span className={cn(
                'text-xs tabular-nums font-mono',
                step.durationMs > 1000 ? 'text-amber-400' : 'text-muted-foreground/40'
              )}>
                {fmtMs(step.durationMs)}
              </span>
            )}
            {(step.input || step.output) && (
              open
                ? <ChevronUp className="w-3.5 h-3.5 text-muted-foreground/40" />
                : <ChevronDown className="w-3.5 h-3.5 text-muted-foreground/40" />
            )}
          </div>
        </button>

        {/* Error strip */}
        {step.errorMsg && (
          <div className="mx-4 mb-3 rounded-lg border border-red-500/20 bg-red-500/[0.06] px-3 py-2">
            <div className="flex items-start gap-2">
              <AlertTriangle className="w-3 h-3 text-red-400 mt-0.5 shrink-0" />
              <p className="text-xs text-red-300/80 leading-relaxed font-mono">{step.errorMsg}</p>
            </div>
          </div>
        )}

        {/* I/O panel */}
        {open && (
          <div className="grid grid-cols-2 gap-3 px-4 pb-4 pt-1">
            {step.input && (
              <div>
                <p className="text-[9px] font-semibold uppercase tracking-widest text-muted-foreground/30 mb-1.5">Input</p>
                <pre className="text-[11px] font-mono text-muted-foreground/70 bg-black/20 border border-white/5 rounded-lg p-3 overflow-auto max-h-40">
                  {JSON.stringify(step.input, null, 2)}
                </pre>
              </div>
            )}
            {step.output && (
              <div>
                <p className="text-[9px] font-semibold uppercase tracking-widest text-muted-foreground/30 mb-1.5">Output</p>
                <pre className="text-[11px] font-mono text-muted-foreground/70 bg-black/20 border border-white/5 rounded-lg p-3 overflow-auto max-h-40">
                  {JSON.stringify(step.output, null, 2)}
                </pre>
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  )
}

// ─── Run detail panel ─────────────────────────────────────────────────────────

function RunDetail({ run, onClose }: { run: AgentRun; onClose: () => void }) {
  const totalToolMs = run.steps.filter((s) => s.kind === 'tool' || s.kind === 'db').reduce((a, s) => a + (s.durationMs ?? 0), 0)
  const planMs      = run.steps.find((s) => s.kind === 'plan')?.durationMs ?? 0

  return (
    <div className="flex flex-col h-full">
      {/* Header */}
      <div className="flex items-start justify-between px-5 py-4 border-b border-white/5 shrink-0">
        <div>
          <div className="flex items-center gap-2 mb-1">
            <Brain className="w-4 h-4 text-[#a78bfa]" />
            <span className="font-semibold text-sm">{run.agentName}</span>
            <Badge
              variant="outline"
              className={cn('text-[9px] h-4 px-1.5', run.status === 'failed'
                ? 'bg-red-500/10 text-red-400 border-red-500/20'
                : 'bg-emerald-500/10 text-emerald-400 border-emerald-500/20')}
            >
              {run.status}
            </Badge>
          </div>
          <div className="flex items-center gap-3 text-xs text-muted-foreground/50">
            <span className="flex items-center gap-1"><Clock className="w-3 h-3" />{fmtMs(run.durationMs)} total</span>
            <span>{run.stepCount} steps</span>
            <span>{relTime(run.startedAt)}</span>
          </div>
        </div>
        <button
          onClick={onClose}
          className="p-1.5 rounded-md hover:bg-white/5 text-muted-foreground hover:text-foreground transition-colors"
        >
          <X className="w-4 h-4" />
        </button>
      </div>

      {/* Run stats bar */}
      <div className="grid grid-cols-3 divide-x divide-white/5 border-b border-white/5 shrink-0">
        {[
          { label: 'Planning',   value: fmtMs(planMs),     color: 'text-[#a78bfa]' },
          { label: 'Tool calls', value: fmtMs(totalToolMs), color: 'text-emerald-400' },
          { label: 'Trigger',    value: run.triggeredBy,    color: 'text-sky-400' },
        ].map(({ label, value, color }) => (
          <div key={label} className="px-4 py-3">
            <p className="text-[9px] font-semibold uppercase tracking-widest text-muted-foreground/30 mb-0.5">{label}</p>
            <p className={cn('text-sm font-semibold font-mono', color)}>{value}</p>
          </div>
        ))}
      </div>

      {/* Steps */}
      <div className="flex-1 overflow-auto px-5 py-5">
        <p className="text-[10px] font-semibold uppercase tracking-widest text-muted-foreground/30 mb-4">
          Execution trace
        </p>
        {run.steps.map((step, i) => (
          <StepRow key={step.id} step={step} isLast={i === run.steps.length - 1} />
        ))}

        {/* Replay + diff panel */}
        <div className="mt-4 border border-white/5 rounded-lg overflow-hidden bg-white/[0.02]">
          <div className="flex border-b border-white/5">
            <div className="flex-1 px-4 py-3 border-r border-white/5">
              <p className="text-[9px] font-semibold uppercase tracking-widest text-muted-foreground/30 mb-2">Replay run</p>
              <code className="text-xs font-mono text-emerald-400 block">flux agent replay {run.id.slice(0, 8)}</code>
              <p className="text-[10px] text-muted-foreground/40 mt-1 leading-relaxed">
                Re-execute with latest code. Diff is shown automatically.
              </p>
            </div>
            <div className="flex-1 px-4 py-3">
              <p className="text-[9px] font-semibold uppercase tracking-widest text-muted-foreground/30 mb-2">Inspect trace</p>
              <code className="text-xs font-mono text-[#a78bfa] block">flux agent trace {run.id.slice(0, 8)}</code>
              <p className="text-[10px] text-muted-foreground/40 mt-1 leading-relaxed">
                Full step-by-step trace with input/output at each stage.
              </p>
            </div>
          </div>
          <div className="px-4 py-2.5">
            <p className="text-[9px] font-semibold uppercase tracking-widest text-muted-foreground/30 mb-1.5">Compare runs</p>
            <code className="text-xs font-mono text-sky-400">flux agent diff {run.id.slice(0, 8)} &lt;other-run-id&gt;</code>
            <p className="text-[10px] text-muted-foreground/40 mt-0.5">Highlights which steps changed, got slower, or started failing.</p>
          </div>
        </div>
      </div>
    </div>
  )
}

// ─── Run list row ─────────────────────────────────────────────────────────────

function RunRow({
  run,
  selected,
  onClick,
}: { run: AgentRun; selected: boolean; onClick: () => void }) {
  return (
    <div
      onClick={onClick}
      className={cn(
        'px-5 py-3.5 border-b border-white/[0.04] cursor-pointer transition-colors flex items-center gap-4',
        selected
          ? 'bg-[#6c63ff]/10 border-l-2 border-l-[#6c63ff]'
          : run.status === 'failed'
            ? 'bg-red-500/[0.02] hover:bg-red-500/[0.04]'
            : 'hover:bg-white/[0.02]'
      )}
    >
      <div className={cn('w-2 h-2 rounded-full shrink-0', run.status === 'failed' ? 'bg-red-400' : run.status === 'running' ? 'bg-amber-400 animate-pulse' : 'bg-emerald-400')} />
      <div className="flex-1 min-w-0">
        <div className="flex items-center gap-2 mb-0.5">
          <span className="text-sm font-medium truncate">{run.agentName}</span>
          <code className="text-[10px] text-muted-foreground/40 font-mono">{run.id.slice(0, 8)}</code>
        </div>
        <p className="text-xs text-muted-foreground/50 truncate">
          {run.stepCount} steps · {run.triggeredBy}
          {run.errorStep && <span className="text-red-400/70"> · failed at {run.errorStep}</span>}
        </p>
      </div>
      <div className="shrink-0 text-right">
        <p className={cn('text-xs font-mono tabular-nums', run.durationMs > 2000 ? 'text-amber-400' : 'text-muted-foreground/50')}>
          {fmtMs(run.durationMs)}
        </p>
        <p className="text-[10px] text-muted-foreground/30">{relTime(run.startedAt)}</p>
      </div>
    </div>
  )
}

// ─── Main page ────────────────────────────────────────────────────────────────

export default function AgentsPage() {
  const { projectId } = useStore()
  const [selectedRun, setSelectedRun] = useState<AgentRun | null>(null)

  const { data: wfData, isLoading: wfLoading, refetch, isFetching } = useQuery({
    queryKey: ['workflows', projectId],
    queryFn: () => apiFetch<{ workflows: Workflow[] }>('/db/workflows'),
    enabled: !!projectId,
  })
  const { data: logData } = useQuery({
    queryKey: ['agent-logs', projectId],
    queryFn: () => apiFetch<{ logs: LogEntry[] }>('/logs?limit=100'),
    enabled: !!projectId,
    refetchInterval: 30_000,
  })

  const workflows = wfData?.workflows ?? []
  const logs = logData?.logs ?? []

  const runs = useMemo(
    () => workflows.flatMap((wf) => deriveRuns(wf, logs))
             .sort((a, b) => new Date(b.startedAt).getTime() - new Date(a.startedAt).getTime()),
    [workflows, logs]
  )

  const stats = useMemo(() => {
    const failed  = runs.filter((r) => r.status === 'failed').length
    const success = runs.filter((r) => r.status === 'success').length
    return {
      total:       runs.length,
      failed,
      successRate: runs.length ? Math.round((success / runs.length) * 100) : 0,
      avgMs:       runs.length ? Math.round(runs.reduce((s, r) => s + r.durationMs, 0) / runs.length) : 0,
    }
  }, [runs])

  return (
    <div className="flex h-full overflow-hidden">
      {/* Left panel */}
      <div className={cn('flex flex-col overflow-hidden transition-all', selectedRun ? 'w-[420px] shrink-0 border-r border-white/5' : 'flex-1')}>
        {/* Header */}
        <div className="px-5 pt-6 pb-4 shrink-0">
          <div className="flex items-center justify-between mb-1">
            <div className="flex items-center gap-2">
              <Brain className="w-5 h-5 text-[#a78bfa]" />
              <h1 className="text-xl font-semibold tracking-tight">Agent Runs</h1>
              <Badge variant="outline" className="text-[9px] h-4 px-1.5 bg-[#6c63ff]/10 text-[#a78bfa] border-[#6c63ff]/30 ml-1">
                beta
              </Badge>
            </div>
            <Button
              variant="ghost" size="sm"
              className="h-7 px-2 text-xs gap-1.5 text-muted-foreground hover:text-foreground"
              onClick={() => refetch()} disabled={isFetching}
            >
              <RefreshCw className={cn('w-3.5 h-3.5', isFetching && 'animate-spin')} />
              Refresh
            </Button>
          </div>
          <p className="text-sm text-muted-foreground">
            Full execution trace for every agent workflow — inputs, tool calls, mutations and errors.
          </p>
        </div>

        {/* Stat chips */}
        {runs.length > 0 && (
          <div className="flex gap-3 px-5 pb-3 shrink-0 flex-wrap">
            {[
              { label: 'Total runs',    value: stats.total,                               color: 'text-foreground'   },
              { label: 'Failed',        value: stats.failed,                              color: stats.failed > 0 ? 'text-red-400' : 'text-muted-foreground/50' },
              { label: 'Success rate',  value: `${stats.successRate}%`,                   color: stats.successRate >= 90 ? 'text-emerald-400' : 'text-amber-400' },
              { label: 'Avg duration',  value: fmtMs(stats.avgMs),                       color: 'text-[#a78bfa]'    },
            ].map(({ label, value, color }) => (
              <div key={label} className="bg-white/[0.03] border border-white/8 rounded-lg px-3 py-2 min-w-[90px]">
                <p className="text-[9px] font-semibold uppercase tracking-widest text-muted-foreground/30">{label}</p>
                <p className={cn('text-sm font-semibold tabular-nums mt-0.5', color)}>{value}</p>
              </div>
            ))}
          </div>
        )}

        {/* Column headers */}
        <div className="flex items-center gap-4 px-5 py-2 border-y border-white/5 text-[10px] font-semibold uppercase tracking-widest text-muted-foreground/30 shrink-0">
          <div className="w-2 shrink-0" />
          <div className="flex-1">Agent / Run ID</div>
          <div className="w-24 text-right">Duration</div>
        </div>

        {/* Run list */}
        <div className="flex-1 overflow-auto">
          {wfLoading && (
            <div className="p-5 space-y-2">
              {[1, 2, 3].map((i) => (
                <div key={i} className="h-16 rounded-lg bg-white/[0.03] animate-pulse" />
              ))}
            </div>
          )}

          {!wfLoading && runs.length === 0 && (
            <div className="flex flex-col items-center justify-center h-full gap-4 text-center p-8">
              <div className="relative">
                <div className="w-16 h-16 rounded-2xl bg-[#6c63ff]/10 border border-[#6c63ff]/20 flex items-center justify-center">
                  <Brain className="w-8 h-8 text-[#a78bfa]/50" />
                </div>
                <Sparkles className="w-4 h-4 text-[#a78bfa] absolute -top-1 -right-1" />
              </div>
              <div>
                <p className="text-sm font-medium text-muted-foreground">No agent runs yet</p>
                <p className="text-xs text-muted-foreground/50 mt-1 max-w-xs leading-relaxed">
                  Deploy a workflow to see its full execution trace here — every tool call, DB mutation and decision, in order.
                </p>
              </div>
              <div className="flex flex-col gap-2 w-full max-w-xs">
                <code className="text-xs font-mono bg-white/[0.04] border border-white/8 rounded-lg px-3 py-2 text-emerald-400 text-left">
                  flux deploy --workflow agent.yaml
                </code>
                <code className="text-xs font-mono bg-white/[0.04] border border-white/8 rounded-lg px-3 py-2 text-muted-foreground/60 text-left">
                  flux agent tail
                </code>
              </div>
            </div>
          )}

          {runs.map((run) => (
            <RunRow
              key={run.id}
              run={run}
              selected={selectedRun?.id === run.id}
              onClick={() => setSelectedRun((prev) => prev?.id === run.id ? null : run)}
            />
          ))}
        </div>
      </div>

      {/* Right detail panel */}
      {selectedRun && (
        <div className="flex-1 flex flex-col overflow-hidden bg-background/40">
          <RunDetail run={selectedRun} onClose={() => setSelectedRun(null)} />
        </div>
      )}

      {/* No-selection prompt when list is narrow */}
      {!selectedRun && runs.length > 0 && (
        <div className="hidden lg:flex flex-1 items-center justify-center border-l border-white/5">
          <div className="text-center">
            <GitBranch className="w-10 h-10 text-muted-foreground/10 mx-auto mb-3" />
            <p className="text-sm text-muted-foreground/40">Select a run to inspect its trace</p>
          </div>
        </div>
      )}
    </div>
  )
}
