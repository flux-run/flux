'use client'

import { useState } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { useParams } from 'next/navigation'
import {
  ListChecks, Play, Trash2, RefreshCw, AlertCircle, Clock,
  CheckCircle2, XCircle, Inbox, ArrowRight, ChevronDown, ChevronRight,
} from 'lucide-react'
import { apiFetch } from '@/lib/api'
import { useStore } from '@/state/tenantStore'
import { PageHeader } from '@/components/layout/PageHeader'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { useToast } from '@/components/ui/toast'
import { cn } from '@/lib/utils'

// ─── Types ────────────────────────────────────────────────────────────────────

interface QueueConfig {
  id: string
  name: string
  concurrency: number
  max_attempts: number
  created_at: string
}

interface QueueJob {
  id: string
  queue_name: string
  status: 'pending' | 'running' | 'done' | 'failed' | 'dlq'
  attempts: number
  max_attempts: number
  payload: unknown
  error?: string
  created_at: string
  started_at?: string
  completed_at?: string
  next_attempt_at?: string
}

interface QueueStats {
  pending: number
  running: number
  done: number
  failed: number
  dlq: number
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

function relTime(ts?: string) {
  if (!ts) return '—'
  const d = Date.now() - new Date(ts).getTime()
  if (d < 60_000)    return `${Math.floor(d / 1000)}s ago`
  if (d < 3_600_000) return `${Math.floor(d / 60_000)}m ago`
  return `${Math.floor(d / 3_600_000)}h ago`
}

const STATUS_BADGE: Record<string, { label: string; cls: string }> = {
  pending: { label: 'Pending', cls: 'bg-zinc-800 text-zinc-300 border-zinc-700' },
  running: { label: 'Running', cls: 'bg-blue-900/60 text-blue-300 border-blue-700' },
  done:    { label: 'Done',    cls: 'bg-emerald-900/60 text-emerald-300 border-emerald-700' },
  failed:  { label: 'Failed',  cls: 'bg-red-900/60 text-red-300 border-red-700' },
  dlq:     { label: 'DLQ',     cls: 'bg-amber-900/60 text-amber-300 border-amber-700' },
}

// ─── QueueCard ────────────────────────────────────────────────────────────────

function QueueCard({ queue, onSelect }: { queue: QueueConfig; onSelect: () => void }) {
  return (
    <button
      onClick={onSelect}
      className="w-full text-left rounded-xl border border-border bg-card p-5 hover:border-border/80 hover:bg-muted/10 transition-colors group"
    >
      <div className="flex items-start justify-between">
        <div>
          <p className="font-medium text-sm">{queue.name}</p>
          <p className="text-xs text-muted-foreground mt-1">
            concurrency: {queue.concurrency} · max attempts: {queue.max_attempts}
          </p>
        </div>
        <ChevronRight className="w-4 h-4 text-muted-foreground group-hover:text-foreground transition-colors mt-0.5" />
      </div>
      <p className="text-xs text-muted-foreground mt-3">Created {relTime(queue.created_at)}</p>
    </button>
  )
}

// ─── JobRow ───────────────────────────────────────────────────────────────────

function JobRow({ job, onRetry, onDelete }: {
  job: QueueJob
  onRetry: (id: string) => void
  onDelete: (id: string) => void
}) {
  const [expanded, setExpanded] = useState(false)
  const s = STATUS_BADGE[job.status] ?? STATUS_BADGE.pending

  return (
    <>
      <tr
        className="border-b border-border/50 hover:bg-muted/5 cursor-pointer"
        onClick={() => setExpanded(e => !e)}
      >
        <td className="py-3 pl-4 pr-2">
          {expanded
            ? <ChevronDown className="w-3.5 h-3.5 text-muted-foreground" />
            : <ChevronRight className="w-3.5 h-3.5 text-muted-foreground" />}
        </td>
        <td className="py-3 pr-4 font-mono text-xs text-muted-foreground">{job.id.slice(0, 8)}</td>
        <td className="py-3 pr-4">
          <span className={cn('inline-flex items-center rounded-full border px-2 py-0.5 text-xs font-medium', s.cls)}>
            {s.label}
          </span>
        </td>
        <td className="py-3 pr-4 text-xs text-muted-foreground">
          {job.attempts}/{job.max_attempts}
        </td>
        <td className="py-3 pr-4 text-xs text-muted-foreground">{relTime(job.created_at)}</td>
        <td className="py-3 pr-4 text-xs text-muted-foreground">{relTime(job.completed_at)}</td>
        <td className="py-3 pr-4" onClick={e => e.stopPropagation()}>
          <div className="flex items-center gap-1.5">
            {(job.status === 'failed' || job.status === 'dlq') && (
              <Button size="sm" variant="ghost" className="h-7 px-2 text-xs" onClick={() => onRetry(job.id)}>
                <RefreshCw className="w-3 h-3 mr-1" /> Retry
              </Button>
            )}
            <Button size="sm" variant="ghost" className="h-7 px-2 text-xs text-red-400 hover:text-red-300" onClick={() => onDelete(job.id)}>
              <Trash2 className="w-3 h-3" />
            </Button>
          </div>
        </td>
      </tr>
      {expanded && (
        <tr className="bg-muted/5 border-b border-border/50">
          <td colSpan={7} className="py-3 px-4">
            <div className="space-y-2">
              <div>
                <p className="text-xs font-medium text-muted-foreground mb-1">Payload</p>
                <pre className="text-xs bg-zinc-950 rounded-lg p-3 overflow-x-auto text-emerald-300 border border-border/40">
                  {JSON.stringify(job.payload, null, 2)}
                </pre>
              </div>
              {job.error && (
                <div>
                  <p className="text-xs font-medium text-red-400 mb-1">Error</p>
                  <pre className="text-xs bg-red-950/30 rounded-lg p-3 text-red-300 border border-red-900/40">
                    {job.error}
                  </pre>
                </div>
              )}
            </div>
          </td>
        </tr>
      )}
    </>
  )
}

// ─── JobsTable ────────────────────────────────────────────────────────────────

function JobsTable({ queueName, status }: { queueName: string; status: string }) {
  const { tenantId } = useStore()
  const qc = useQueryClient()
  const { toast } = useToast()
  const params = useParams<{ projectId: string }>()

  const { data: jobs = [], isLoading } = useQuery<QueueJob[]>({
    queryKey: ['queue-jobs', queueName, status, tenantId],
    queryFn: () => apiFetch(`/flux/api/queues/${queueName}/jobs?status=${status}`),
    refetchInterval: status === 'running' || status === 'pending' ? 5000 : false,
  })

  const retryMut = useMutation({
    mutationFn: (jobId: string) =>
      apiFetch(`/flux/api/queues/${queueName}/jobs/${jobId}/retry`, { method: 'POST' }),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['queue-jobs', queueName] })
      toast({ title: 'Job queued for retry', variant: 'success' })
    },
    onError: () => toast({ title: 'Failed to retry job', variant: 'error' }),
  })

  const deleteMut = useMutation({
    mutationFn: (jobId: string) =>
      apiFetch(`/flux/api/queues/${queueName}/jobs/${jobId}`, { method: 'DELETE' }),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['queue-jobs', queueName] })
      toast({ title: 'Job deleted', variant: 'success' })
    },
    onError: () => toast({ title: 'Failed to delete job', variant: 'error' }),
  })

  if (isLoading) {
    return (
      <div className="py-12 text-center text-muted-foreground text-sm animate-pulse">
        Loading jobs…
      </div>
    )
  }

  if (jobs.length === 0) {
    return (
      <div className="py-16 text-center">
        <Inbox className="w-8 h-8 text-muted-foreground/40 mx-auto mb-3" />
        <p className="text-sm text-muted-foreground">No {status} jobs</p>
      </div>
    )
  }

  return (
    <div className="overflow-x-auto">
      <table className="w-full text-sm">
        <thead>
          <tr className="border-b border-border text-left">
            <th className="pb-2 pl-4 pr-2 w-6" />
            <th className="pb-2 pr-4 text-xs font-medium text-muted-foreground">Job ID</th>
            <th className="pb-2 pr-4 text-xs font-medium text-muted-foreground">Status</th>
            <th className="pb-2 pr-4 text-xs font-medium text-muted-foreground">Attempts</th>
            <th className="pb-2 pr-4 text-xs font-medium text-muted-foreground">Created</th>
            <th className="pb-2 pr-4 text-xs font-medium text-muted-foreground">Completed</th>
            <th className="pb-2 pr-4 text-xs font-medium text-muted-foreground">Actions</th>
          </tr>
        </thead>
        <tbody>
          {jobs.map(job => (
            <JobRow
              key={job.id}
              job={job}
              onRetry={id => retryMut.mutate(id)}
              onDelete={id => deleteMut.mutate(id)}
            />
          ))}
        </tbody>
      </table>
    </div>
  )
}

// ─── StatsBar ─────────────────────────────────────────────────────────────────

function StatsBar({ stats }: { stats?: QueueStats }) {
  if (!stats) return null
  return (
    <div className="flex items-center gap-4 text-xs text-muted-foreground">
      <span className="flex items-center gap-1.5">
        <span className="w-2 h-2 rounded-full bg-zinc-500" />
        {stats.pending} pending
      </span>
      <span className="flex items-center gap-1.5">
        <span className="w-2 h-2 rounded-full bg-blue-400 animate-pulse" />
        {stats.running} running
      </span>
      <span className="flex items-center gap-1.5">
        <span className="w-2 h-2 rounded-full bg-emerald-400" />
        {stats.done} done
      </span>
      <span className="flex items-center gap-1.5">
        <span className="w-2 h-2 rounded-full bg-red-400" />
        {stats.failed} failed
      </span>
      <span className="flex items-center gap-1.5">
        <span className="w-2 h-2 rounded-full bg-amber-400" />
        {stats.dlq} DLQ
      </span>
    </div>
  )
}

// ─── Main Page ────────────────────────────────────────────────────────────────

export default function QueuePage() {
  const { tenantId } = useStore()
  const { toast } = useToast()
  const qc = useQueryClient()
  const [selected, setSelected] = useState<QueueConfig | null>(null)
  const [activeTab, setActiveTab] = useState('pending')

  const { data: queues = [], isLoading } = useQuery<QueueConfig[]>({
    queryKey: ['queues', tenantId],
    queryFn: () => apiFetch('/flux/api/queues'),
  })

  const { data: stats } = useQuery<QueueStats>({
    queryKey: ['queue-stats', selected?.name, tenantId],
    queryFn: () => apiFetch(`/flux/api/queues/${selected!.name}/stats`),
    enabled: !!selected,
    refetchInterval: 8000,
  })

  const purgeMut = useMutation({
    mutationFn: () =>
      apiFetch(`/flux/api/queues/${selected!.name}/purge`, { method: 'POST' }),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['queue-jobs', selected!.name] })
      qc.invalidateQueries({ queryKey: ['queue-stats', selected!.name] })
      toast({ title: `Queue "${selected!.name}" purged`, variant: 'success' })
    },
    onError: () => toast({ title: 'Purge failed', variant: 'error' }),
  })

  const promoteDlqMut = useMutation({
    mutationFn: () =>
      apiFetch(`/flux/api/queues/${selected!.name}/dlq/promote`, { method: 'POST' }),
    onSuccess: (data: any) => {
      qc.invalidateQueries({ queryKey: ['queue-jobs', selected!.name] })
      qc.invalidateQueries({ queryKey: ['queue-stats', selected!.name] })
      toast({ title: `Promoted ${data?.count ?? 0} DLQ jobs`, variant: 'success' })
    },
    onError: () => toast({ title: 'Promote failed', variant: 'error' }),
  })

  if (selected) {
    return (
      <div className="space-y-6">
        <PageHeader
          title={selected.name}
          description={`concurrency: ${selected.concurrency} · max attempts: ${selected.max_attempts}`}
          actions={
            <div className="flex items-center gap-2">
              <Button
                size="sm" variant="outline"
                onClick={() => setSelected(null)}
              >
                ← All Queues
              </Button>
              <Button
                size="sm" variant="outline"
                onClick={() => promoteDlqMut.mutate()}
                disabled={promoteDlqMut.isPending || (stats?.dlq ?? 0) === 0}
              >
                <ArrowRight className="w-3.5 h-3.5 mr-1.5" />
                Promote DLQ
              </Button>
              <Button
                size="sm" variant="outline"
                className="text-red-400 border-red-900/40 hover:bg-red-900/20"
                onClick={() => purgeMut.mutate()}
                disabled={purgeMut.isPending}
              >
                <Trash2 className="w-3.5 h-3.5 mr-1.5" />
                Purge
              </Button>
            </div>
          }
        />

        <StatsBar stats={stats} />

        <div className="rounded-xl border border-border bg-card">
          <Tabs value={activeTab} onValueChange={setActiveTab}>
            <div className="border-b border-border px-4">
              <TabsList className="h-10 bg-transparent gap-1 p-0">
                {['pending', 'running', 'done', 'failed', 'dlq'].map(s => (
                  <TabsTrigger
                    key={s}
                    value={s}
                    className="rounded-none border-b-2 border-transparent data-[state=active]:border-foreground pb-2 pt-2 px-3 text-xs font-medium capitalize"
                  >
                    {s}
                    {stats && (
                      <span className="ml-1.5 text-muted-foreground tabular-nums">
                        {stats[s as keyof QueueStats]}
                      </span>
                    )}
                  </TabsTrigger>
                ))}
              </TabsList>
            </div>
            {['pending', 'running', 'done', 'failed', 'dlq'].map(s => (
              <TabsContent key={s} value={s} className="mt-0">
                <JobsTable queueName={selected.name} status={s} />
              </TabsContent>
            ))}
          </Tabs>
        </div>
      </div>
    )
  }

  return (
    <div className="space-y-6">
      <PageHeader
        title="Queues"
        description="Inspect job queues, retry failed jobs, and manage dead-letter queues."
        actions={
          <Button
            size="sm" variant="ghost"
            onClick={() => qc.invalidateQueries({ queryKey: ['queues'] })}
          >
            <RefreshCw className="w-3.5 h-3.5 mr-1.5" /> Refresh
          </Button>
        }
      />

      {isLoading ? (
        <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4">
          {[1, 2, 3].map(i => (
            <div key={i} className="rounded-xl border border-border bg-card p-5 h-28 animate-pulse" />
          ))}
        </div>
      ) : queues.length === 0 ? (
        <div className="rounded-xl border border-dashed border-border bg-card/40 p-12 text-center">
          <ListChecks className="w-8 h-8 text-muted-foreground/40 mx-auto mb-3" />
          <p className="text-sm font-medium">No queues configured</p>
          <p className="text-xs text-muted-foreground mt-1">
            Queues are created when a subscription or workflow step targets a queue.
          </p>
        </div>
      ) : (
        <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4">
          {queues.map(q => (
            <QueueCard key={q.id} queue={q} onSelect={() => setSelected(q)} />
          ))}
        </div>
      )}
    </div>
  )
}
