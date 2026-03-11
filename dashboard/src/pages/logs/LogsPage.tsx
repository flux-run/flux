import { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { apiFetch } from '@/lib/api'
import { useStore } from '@/state/tenantStore'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import {
  RefreshCw, Terminal, GitBranch, Clock, Info, AlertTriangle,
  AlertCircle, Bug, ChevronRight, CheckCircle2, XCircle,
} from 'lucide-react'
import { cn } from '@/lib/utils'

// ─── Types ────────────────────────────────────────────────────────────────────

interface Fn {
  id: string
  name: string
  runtime: string
}

interface LogEntry {
  id: string
  level: string
  message: string
  timestamp: string
}

interface CronJob {
  id: string
  name: string
  schedule: string
  action_type: string
  enabled: boolean
  last_run_at: string | null
  next_run_at: string | null
}

interface Workflow {
  id: string
  name: string
  trigger_event: string
  enabled: boolean
  steps: { action_type: string; config?: Record<string, unknown> }[]
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

const LEVEL_STYLE: Record<string, string> = {
  info:  'text-sky-400',
  warn:  'text-amber-400',
  error: 'text-red-400',
  debug: 'text-muted-foreground',
}

const LEVEL_BADGE: Record<string, string> = {
  info:  'bg-sky-500/15 text-sky-400 border-sky-500/20',
  warn:  'bg-amber-500/15 text-amber-400 border-amber-500/20',
  error: 'bg-red-500/15 text-red-400 border-red-500/20',
  debug: 'bg-white/5 text-muted-foreground border-white/10',
}

function LevelIcon({ level }: { level: string }) {
  const cls = 'w-3.5 h-3.5 shrink-0'
  switch (level) {
    case 'info':  return <Info className={cn(cls, 'text-sky-400')} />
    case 'warn':  return <AlertTriangle className={cn(cls, 'text-amber-400')} />
    case 'error': return <AlertCircle className={cn(cls, 'text-red-400')} />
    case 'debug': return <Bug className={cn(cls, 'text-muted-foreground')} />
    default:      return <Info className={cn(cls, 'text-muted-foreground')} />
  }
}

function formatTs(ts: string | null): string {
  if (!ts) return '—'
  try {
    return new Date(ts).toLocaleString(undefined, {
      year: 'numeric', month: 'short', day: '2-digit',
      hour: '2-digit', minute: '2-digit', second: '2-digit',
    })
  } catch {
    return ts
  }
}

// ─── Function Logs Tab ────────────────────────────────────────────────────────

function FunctionLogsTab() {
  const { projectId } = useStore()
  const [functionId, setFunctionId] = useState('all')
  const [limit, setLimit] = useState('50')
  const [filterLevel, setFilterLevel] = useState('all')

  const { data: fnData } = useQuery({
    queryKey: ['functions', projectId],
    queryFn: () => apiFetch<{ functions: Fn[] }>('/functions'),
    enabled: !!projectId,
  })

  const {
    data: logsData,
    isFetching,
    refetch,
    error: logsError,
  } = useQuery({
    queryKey: ['function-logs', projectId, functionId, limit],
    queryFn: async () => {
      const params = new URLSearchParams({ limit })
      if (functionId && functionId !== 'all') params.set('function_id', functionId)

      return apiFetch<{ logs: LogEntry[] }>(`/logs?${params.toString()}`)
    },
    enabled: !!projectId,
  })

  const logs = logsData?.logs ?? []
  const visible = filterLevel === 'all' ? logs : logs.filter((l) => l.level === filterLevel)

  return (
    <div className="flex-1 flex flex-col overflow-hidden">
      {/* Toolbar */}
      <div className="flex items-center gap-3 px-5 py-3 border-b border-white/5 shrink-0 flex-wrap">
        <div className="flex items-center gap-2">
          <Label className="text-xs text-muted-foreground whitespace-nowrap">Function</Label>
          <Select value={functionId} onValueChange={setFunctionId}>
            <SelectTrigger className="h-7 text-xs w-48 bg-white/5 border-white/10">
              <SelectValue placeholder="All functions" />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="all" className="text-xs">All functions</SelectItem>
              {(fnData?.functions ?? []).map((f) => (
                <SelectItem key={f.id} value={f.id} className="text-xs">{f.name}</SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>

        <div className="flex items-center gap-2">
          <Label className="text-xs text-muted-foreground">Level</Label>
          <Select value={filterLevel} onValueChange={setFilterLevel}>
            <SelectTrigger className="h-7 text-xs w-28 bg-white/5 border-white/10">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {['all', 'info', 'warn', 'error', 'debug'].map((l) => (
                <SelectItem key={l} value={l} className="text-xs capitalize">{l}</SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>

        <div className="flex items-center gap-2">
          <Label className="text-xs text-muted-foreground">Limit</Label>
          <Input
            value={limit}
            onChange={(e) => setLimit(e.target.value)}
            type="number"
            min="10"
            max="500"
            className="h-7 text-xs w-20 bg-white/5 border-white/10"
          />
        </div>

        <Button
          variant="ghost"
          size="sm"
          className="h-7 px-2 ml-auto text-xs gap-1.5 text-muted-foreground hover:text-foreground"
          onClick={() => refetch()}
          disabled={isFetching}
        >
          <RefreshCw className={cn('w-3.5 h-3.5', isFetching && 'animate-spin')} />
          Refresh
        </Button>
      </div>

      {/* Error */}
      {logsError && (
        <div className="mx-5 mt-4 rounded-lg border border-destructive/30 bg-destructive/10 px-4 py-3 shrink-0">
          <p className="text-xs font-medium text-destructive">{(logsError as Error).message}</p>
        </div>
      )}

      {/* Log list */}
      <div className="flex-1 overflow-auto font-mono text-xs">
        {visible.length === 0 && !isFetching && !logsError && (
          <div className="flex flex-col items-center justify-center h-full text-center p-8">
            <Terminal className="w-8 h-8 text-muted-foreground/30 mb-3" />
            <p className="text-sm text-muted-foreground">No logs found</p>
            <p className="text-xs text-muted-foreground/50 mt-1">Invoke a function to generate logs</p>
          </div>
        )}
        {visible.map((log) => (
          <div
            key={log.id}
            className={cn(
              'flex items-start gap-3 px-5 py-2 border-b border-white/[0.04] hover:bg-white/[0.02] transition-colors',
              log.level === 'error' && 'bg-red-500/[0.03]'
            )}
          >
            <LevelIcon level={log.level} />
            <span className="text-muted-foreground/50 text-[10px] mt-0.5 shrink-0 w-44">{formatTs(log.timestamp)}</span>
            <Badge variant="outline" className={cn('text-[9px] h-4 px-1.5 shrink-0 border', LEVEL_BADGE[log.level] ?? LEVEL_BADGE.debug)}>
              {log.level}
            </Badge>
            <span className={cn('flex-1 break-all', LEVEL_STYLE[log.level] ?? 'text-foreground')}>
              {log.message}
            </span>
          </div>
        ))}
      </div>

      {/* Footer count */}
      {visible.length > 0 && (
        <div className="px-5 py-1.5 border-t border-white/5 shrink-0">
          <p className="text-[10px] text-muted-foreground/40">{visible.length} log entr{visible.length === 1 ? 'y' : 'ies'}</p>
        </div>
      )}
    </div>
  )
}

// ─── Workflows Tab ────────────────────────────────────────────────────────────

function WorkflowsTab() {
  const { projectId } = useStore()

  const { data, isFetching, refetch } = useQuery({
    queryKey: ['workflows-log', projectId],
    queryFn: () => apiFetch<{ workflows: Workflow[] }>('/db/workflows'),
    enabled: !!projectId,
  })

  const workflows = data?.workflows ?? []

  return (
    <div className="flex-1 flex flex-col overflow-hidden">
      <div className="flex items-center justify-between px-5 py-3 border-b border-white/5 shrink-0">
        <p className="text-xs text-muted-foreground">{workflows.length} workflow{workflows.length !== 1 ? 's' : ''} registered</p>
        <Button variant="ghost" size="sm" className="h-7 px-2 text-xs gap-1.5 text-muted-foreground" onClick={() => refetch()} disabled={isFetching}>
          <RefreshCw className={cn('w-3.5 h-3.5', isFetching && 'animate-spin')} />
          Refresh
        </Button>
      </div>
      <div className="flex-1 overflow-auto">
        {workflows.length === 0 && !isFetching && (
          <div className="flex flex-col items-center justify-center h-full p-8 text-center">
            <GitBranch className="w-8 h-8 text-muted-foreground/30 mb-3" />
            <p className="text-sm text-muted-foreground">No workflows</p>
          </div>
        )}
        {workflows.map((wf) => (
          <div key={wf.id} className="px-5 py-3.5 border-b border-white/5 hover:bg-white/[0.02] transition-colors">
            <div className="flex items-center gap-3 mb-2">
              {wf.enabled
                ? <CheckCircle2 className="w-3.5 h-3.5 text-emerald-400 shrink-0" />
                : <XCircle className="w-3.5 h-3.5 text-muted-foreground/40 shrink-0" />}
              <span className="text-sm font-medium">{wf.name}</span>
              <Badge variant="outline" className="text-[10px] border-white/10 bg-white/5 text-muted-foreground">
                {wf.trigger_event || 'no trigger'}
              </Badge>
            </div>
            <div className="flex items-center gap-1.5 flex-wrap ml-6">
              {wf.steps.map((step, i) => (
                <div key={i} className="flex items-center gap-1">
                  {i > 0 && <ChevronRight className="w-3 h-3 text-muted-foreground/30" />}
                  <Badge variant="outline" className="text-[10px] border-white/10 bg-white/5 text-sky-400">
                    {step.action_type}
                  </Badge>
                </div>
              ))}
            </div>
          </div>
        ))}
      </div>
    </div>
  )
}

// ─── Cron Tab ─────────────────────────────────────────────────────────────────

function CronTab() {
  const { projectId } = useStore()

  const { data, isFetching, refetch } = useQuery({
    queryKey: ['cron-log', projectId],
    queryFn: () => apiFetch<{ cron: CronJob[] }>('/db/cron'),
    enabled: !!projectId,
  })

  const jobs = data?.cron ?? []

  return (
    <div className="flex-1 flex flex-col overflow-hidden">
      <div className="flex items-center justify-between px-5 py-3 border-b border-white/5 shrink-0">
        <p className="text-xs text-muted-foreground">{jobs.length} scheduled job{jobs.length !== 1 ? 's' : ''}</p>
        <Button variant="ghost" size="sm" className="h-7 px-2 text-xs gap-1.5 text-muted-foreground" onClick={() => refetch()} disabled={isFetching}>
          <RefreshCw className={cn('w-3.5 h-3.5', isFetching && 'animate-spin')} />
          Refresh
        </Button>
      </div>
      <div className="flex-1 overflow-auto">
        {jobs.length === 0 && !isFetching && (
          <div className="flex flex-col items-center justify-center h-full p-8 text-center">
            <Clock className="w-8 h-8 text-muted-foreground/30 mb-3" />
            <p className="text-sm text-muted-foreground">No cron jobs</p>
          </div>
        )}
        <table className="w-full text-xs">
          {jobs.length > 0 && (
            <thead>
              <tr className="border-b border-white/5">
                {['Name', 'Schedule', 'Action', 'Last Run', 'Next Run', 'Status'].map((h) => (
                  <th key={h} className="text-left px-5 py-2.5 font-semibold text-muted-foreground whitespace-nowrap">{h}</th>
                ))}
              </tr>
            </thead>
          )}
          <tbody>
            {jobs.map((job) => (
              <tr key={job.id} className="border-b border-white/[0.04] hover:bg-white/[0.02] transition-colors">
                <td className="px-5 py-3 font-medium">{job.name}</td>
                <td className="px-5 py-3 font-mono text-muted-foreground">{job.schedule}</td>
                <td className="px-5 py-3">
                  <Badge variant="outline" className="text-[10px] border-white/10 bg-white/5 text-sky-400">
                    {job.action_type}
                  </Badge>
                </td>
                <td className="px-5 py-3 text-muted-foreground/70 whitespace-nowrap">{formatTs(job.last_run_at)}</td>
                <td className="px-5 py-3 text-muted-foreground/70 whitespace-nowrap">{formatTs(job.next_run_at)}</td>
                <td className="px-5 py-3">
                  {job.enabled
                    ? <span className="flex items-center gap-1 text-emerald-400"><CheckCircle2 className="w-3 h-3" /> Active</span>
                    : <span className="flex items-center gap-1 text-muted-foreground/40"><XCircle className="w-3 h-3" /> Disabled</span>}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  )
}

// ─── Main ─────────────────────────────────────────────────────────────────────

export default function LogsPage() {
  return (
    <div className="flex flex-col h-full overflow-hidden">
      {/* Header */}
      <div className="px-6 pt-6 pb-4 shrink-0">
        <h1 className="text-xl font-semibold tracking-tight">Logs Explorer</h1>
        <p className="text-sm text-muted-foreground mt-0.5">
          Inspect function logs, monitor workflows, and track cron jobs.
        </p>
      </div>

      {/* Tabs */}
      <Tabs defaultValue="functions" className="flex-1 flex flex-col overflow-hidden px-6 pb-4">
        <TabsList className="h-8 w-fit shrink-0">
          <TabsTrigger value="functions" className="text-xs h-7 gap-1.5 px-3">
            <Terminal className="w-3.5 h-3.5" /> Function Logs
          </TabsTrigger>
          <TabsTrigger value="workflows" className="text-xs h-7 gap-1.5 px-3">
            <GitBranch className="w-3.5 h-3.5" /> Workflows
          </TabsTrigger>
          <TabsTrigger value="cron" className="text-xs h-7 gap-1.5 px-3">
            <Clock className="w-3.5 h-3.5" /> Cron Jobs
          </TabsTrigger>
        </TabsList>

        <div className="flex-1 overflow-hidden mt-4 rounded-xl border border-white/5 bg-background/40 flex flex-col">
          <TabsContent value="functions" className="flex-1 flex flex-col overflow-hidden m-0 data-[state=active]:flex">
            <FunctionLogsTab />
          </TabsContent>
          <TabsContent value="workflows" className="flex-1 flex flex-col overflow-hidden m-0 data-[state=active]:flex">
            <WorkflowsTab />
          </TabsContent>
          <TabsContent value="cron" className="flex-1 flex flex-col overflow-hidden m-0 data-[state=active]:flex">
            <CronTab />
          </TabsContent>
        </div>
      </Tabs>
    </div>
  )
}
