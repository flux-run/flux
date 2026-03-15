'use client'

import { useState } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { useParams } from 'next/navigation'
import { Clock, Plus, Trash2, Play } from 'lucide-react'
import { apiFetch, gatewayFetch } from '@/lib/api'
import type { CronJobRow } from '@flux/api-types'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Badge } from '@/components/ui/badge'
import {
  Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription, DialogFooter,
} from '@/components/ui/dialog'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { PageHeader } from '@/components/layout/PageHeader'

function fmtDate(d: string | null): string {
  if (!d) return '—'
  return new Date(d).toLocaleString(undefined, { dateStyle: 'short', timeStyle: 'short' })
}

const PRESETS: { label: string; value: string }[] = [
  { label: 'Every minute',       value: '* * * * *'   },
  { label: 'Every 5 minutes',    value: '*/5 * * * *' },
  { label: 'Every hour',         value: '0 * * * *'   },
  { label: 'Every day at 9am',   value: '0 9 * * *'   },
  { label: 'Every Monday 9am',   value: '0 9 * * 1'   },
  { label: 'Custom…',            value: '__custom__'  },
]

export default function CronPage() {
  const queryClient = useQueryClient()
  const [createOpen, setCreateOpen] = useState(false)
  const [preset, setPreset] = useState('0 * * * *')
  const [customSchedule, setCustomSchedule] = useState('')
  const [form, setForm] = useState({ name: '', action_type: 'function', action_value: '' })

  const { data, isLoading } = useQuery({
    queryKey: ['cron'],
    queryFn: () => apiFetch<{ cron: CronJobRow[] }>('/db/cron'),
  })

  const deleteMutation = useMutation({
    mutationFn: (id: string) => apiFetch(`/db/cron/${id}`, { method: 'DELETE' }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['cron'] }),
  })

  const toggleMutation = useMutation({
    mutationFn: ({ id, enabled }: { id: string; enabled: boolean }) =>
      apiFetch(`/db/cron/${id}`, { method: 'PATCH', body: JSON.stringify({ enabled }) }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['cron'] }),
  })

  const createMutation = useMutation({
    mutationFn: () => {
      const schedule = preset === '__custom__' ? customSchedule : preset
      return apiFetch('/db/cron', {
        method: 'POST',
        body: JSON.stringify({
          name: form.name,
          schedule,
          action_type: form.action_type,
          action_config: { value: form.action_value },
        }),
      })
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['cron'] })
      setCreateOpen(false)
      setForm({ name: '', action_type: 'function', action_value: '' })
    },
  })

  const runNowMutation = useMutation({
    mutationFn: (id: string) => gatewayFetch(`/db/cron/${id}/trigger`, { method: 'POST' }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['cron'] }),
  })

  const jobs = data?.cron ?? []

  return (
    <div className="flex flex-col h-full">
      <PageHeader
        title="Cron Jobs"
        description={jobs.length > 0 ? `${jobs.length} scheduled` : 'Scheduled functions and queue jobs'}
        breadcrumbs={[
          { label: 'Projects', href: '/dashboard' },
          { label: 'Cron' },
        ]}
        actions={
          <Button size="sm" onClick={() => setCreateOpen(true)}>
            <Plus className="w-3.5 h-3.5" /> New job
          </Button>
        }
      />
      <div className="flex-1 overflow-y-auto">
      <div className="p-6 max-w-5xl mx-auto">

      {isLoading ? (
        <div className="space-y-2">
          {[...Array(3)].map((_, i) => (
            <div key={i} className="h-16 rounded-xl border bg-card animate-pulse" />
          ))}
        </div>
      ) : jobs.length === 0 ? (
        <div className="flex flex-col items-center justify-center py-20 text-center border rounded-xl bg-card">
          <Clock className="w-10 h-10 text-muted-foreground/40 mb-3" />
          <p className="font-medium text-sm">No cron jobs</p>
          <p className="text-xs text-muted-foreground mt-1 mb-4">
            Schedule functions and queue jobs to run automatically on a time interval.
          </p>
          <Button size="sm" variant="outline" onClick={() => setCreateOpen(true)}>
            <Plus className="w-4 h-4 mr-1.5" /> Create cron job
          </Button>
        </div>
      ) : (
        <div className="rounded-xl border overflow-hidden bg-card">
          <div className="grid grid-cols-[1fr_160px_140px_140px_120px] gap-4 px-5 py-2.5 bg-muted/30 border-b">
            {['Name / Schedule', 'Action', 'Last run', 'Next run', 'Status'].map((h) => (
              <p key={h} className="text-[10px] font-semibold uppercase tracking-widest text-muted-foreground/70">{h}</p>
            ))}
          </div>
          {jobs.map((job) => (
            <div
              key={job.id}
              className="group grid grid-cols-[1fr_160px_140px_140px_120px] gap-4 px-5 py-3.5 border-b last:border-0 items-center hover:bg-muted/20 transition-colors"
            >
              <div>
                <p className="font-medium text-sm">{job.name}</p>
                <p className="text-[11px] font-mono text-muted-foreground">{job.schedule}</p>
              </div>
              <Badge variant="secondary" className="text-[10px] w-fit">{job.action_type}</Badge>
              <p className="text-xs text-muted-foreground">{fmtDate(job.last_run_at)}</p>
              <p className="text-xs text-muted-foreground">{fmtDate(job.next_run_at)}</p>
              <div className="flex items-center gap-1.5">
                <button
                  onClick={() => toggleMutation.mutate({ id: job.id, enabled: !job.enabled })}
                  className={`relative inline-flex h-4 w-8 items-center rounded-full transition-colors ${job.enabled ? 'bg-primary' : 'bg-muted'}`}
                >
                  <span className={`inline-block h-3 w-3 rounded-full bg-white transition-transform ${job.enabled ? 'translate-x-4.5' : 'translate-x-0.5'}`} />
                </button>
                <Button
                  variant="ghost" size="icon" className="w-6 h-6 opacity-0 group-hover:opacity-100 transition-opacity text-emerald-400 hover:text-emerald-300 hover:bg-emerald-500/10"
                  onClick={() => runNowMutation.mutate(job.id)}
                  title="Run now"
                  disabled={runNowMutation.isPending}
                >
                  <Play className="w-3 h-3" />
                </Button>
                <Button
                  variant="ghost" size="icon" className="w-6 h-6 opacity-0 group-hover:opacity-100 transition-opacity"
                  onClick={() => deleteMutation.mutate(job.id)}
                >
                  <Trash2 className="w-3 h-3 text-muted-foreground" />
                </Button>
              </div>
            </div>
          ))}
        </div>
      )}

      {/* Create dialog */}
      <Dialog open={createOpen} onOpenChange={setCreateOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>New cron job</DialogTitle>
            <DialogDescription>Schedule a function or queue job to run automatically.</DialogDescription>
          </DialogHeader>
          <div className="space-y-3 py-2">
            <div className="space-y-1.5">
              <Label>Name</Label>
              <Input
                placeholder="e.g. send_weekly_digest"
                value={form.name}
                onChange={(e) => setForm((f) => ({ ...f, name: e.target.value }))}
              />
            </div>
            <div className="space-y-1.5">
              <Label>Schedule</Label>
              <Select value={preset} onValueChange={setPreset}>
                <SelectTrigger><SelectValue /></SelectTrigger>
                <SelectContent>
                  {PRESETS.map((p) => (
                    <SelectItem key={p.value} value={p.value}>{p.label}</SelectItem>
                  ))}
                </SelectContent>
              </Select>
              {preset === '__custom__' && (
                <Input
                  placeholder="cron expression (e.g. 0 9 * * 1)"
                  className="font-mono text-xs mt-1.5"
                  value={customSchedule}
                  onChange={(e) => setCustomSchedule(e.target.value)}
                />
              )}
              <p className="text-[10px] text-muted-foreground">
                {preset !== '__custom__' ? `Expression: ${preset}` : '5-field cron: min hour dom month dow'}
              </p>
            </div>
            <div className="grid grid-cols-2 gap-3">
              <div className="space-y-1.5">
                <Label>Action type</Label>
                <Select value={form.action_type} onValueChange={(v) => setForm((f) => ({ ...f, action_type: v }))}>
                  <SelectTrigger><SelectValue /></SelectTrigger>
                  <SelectContent>
                    <SelectItem value="function">Function</SelectItem>
                    <SelectItem value="queue_job">Queue job</SelectItem>
                  </SelectContent>
                </Select>
              </div>
              <div className="space-y-1.5">
                <Label>Function / Job ID</Label>
                <Input
                  placeholder="function-uuid or job-name"
                  value={form.action_value}
                  onChange={(e) => setForm((f) => ({ ...f, action_value: e.target.value }))}
                />
              </div>
            </div>
          </div>
          <DialogFooter>
            <Button variant="ghost" onClick={() => setCreateOpen(false)}>Cancel</Button>
            <Button
              onClick={() => createMutation.mutate()}
              disabled={!form.name || !form.action_value || createMutation.isPending}
            >
              {createMutation.isPending ? 'Saving…' : 'Create job'}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
      </div>
      </div>
    </div>
  )
}
