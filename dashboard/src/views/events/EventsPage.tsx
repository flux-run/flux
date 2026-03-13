'use client'

import { useState } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { useParams } from 'next/navigation'
import { Bell, Plus, Trash2, Radio } from 'lucide-react'
import { apiFetch } from '@/lib/api'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Badge } from '@/components/ui/badge'
import {
  Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription, DialogFooter,
} from '@/components/ui/dialog'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'

interface Subscription {
  id: string
  event_pattern: string
  target_type: string
  target_config: Record<string, unknown>
  enabled: boolean
}

interface SubResponse { subscriptions: Subscription[] }

const TARGET_COLOR: Record<string, string> = {
  webhook:      'bg-sky-500/10 text-sky-700 dark:text-sky-400',
  function:     'bg-purple-500/10 text-purple-700 dark:text-purple-400',
  queue_job:    'bg-amber-500/10 text-amber-700 dark:text-amber-400',
}

export default function EventsPage() {
  const { projectId } = useParams() as any
  const queryClient = useQueryClient()
  const [createOpen, setCreateOpen] = useState(false)
  const [form, setForm] = useState({
    event_pattern: '',
    target_type: 'webhook',
    target_url: '',
  })

  const { data, isLoading } = useQuery({
    queryKey: ['subscriptions', projectId],
    queryFn: () => apiFetch<SubResponse>('/db/subscriptions'),
    enabled: !!projectId,
  })

  const deleteMutation = useMutation({
    mutationFn: (id: string) => apiFetch(`/db/subscriptions/${id}`, { method: 'DELETE' }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['subscriptions'] }),
  })

  const createMutation = useMutation({
    mutationFn: () =>
      apiFetch('/db/subscriptions', {
        method: 'POST',
        body: JSON.stringify({
          event_pattern: form.event_pattern,
          target_type: form.target_type,
          target_config: { url: form.target_url },
        }),
      }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['subscriptions'] })
      setCreateOpen(false)
      setForm({ event_pattern: '', target_type: 'webhook', target_url: '' })
    },
  })

  const subs = data?.subscriptions ?? []

  return (
    <div className="p-8 max-w-5xl mx-auto">
      <div className="flex items-center justify-between mb-8">
        <div>
          <h1 className="text-2xl font-bold">Events</h1>
          <p className="text-sm text-muted-foreground mt-0.5">
            Subscribe to database change events and route them to functions, webhooks, or queues
          </p>
        </div>
        <Button size="sm" onClick={() => setCreateOpen(true)}>
          <Plus className="w-4 h-4 mr-1.5" /> New subscription
        </Button>
      </div>

      {isLoading ? (
        <div className="space-y-2">
          {[...Array(4)].map((_, i) => (
            <div key={i} className="h-16 rounded-xl border bg-card animate-pulse" />
          ))}
        </div>
      ) : subs.length === 0 ? (
        <div className="flex flex-col items-center justify-center py-20 text-center border rounded-xl bg-card">
          <Radio className="w-10 h-10 text-muted-foreground/40 mb-3" />
          <p className="font-medium text-sm">No event subscriptions</p>
          <p className="text-xs text-muted-foreground mt-1 mb-4">
            Subscriptions forward database events to webhooks, functions, or job queues.
            <br />
            Example patterns: <code className="text-xs">users.inserted</code>, <code className="text-xs">orders.*</code>, <code className="text-xs">*</code>
          </p>
          <Button size="sm" variant="outline" onClick={() => setCreateOpen(true)}>
            <Plus className="w-4 h-4 mr-1.5" /> Add subscription
          </Button>
        </div>
      ) : (
        <div className="rounded-xl border divide-y overflow-hidden bg-card">
          {subs.map((s) => (
            <div key={s.id} className="group flex items-center justify-between px-5 py-4 hover:bg-muted/20 transition-colors">
              <div className="flex items-center gap-3">
                <Bell className="w-4 h-4 text-muted-foreground shrink-0" />
                <div>
                  <div className="flex items-center gap-2">
                    <p className="font-mono text-sm font-medium">{s.event_pattern}</p>
                    <span className={`text-[10px] font-semibold px-2 py-0.5 rounded-full ${TARGET_COLOR[s.target_type] ?? 'bg-muted'}`}>
                      {s.target_type}
                    </span>
                    {!s.enabled && (
                      <Badge variant="secondary" className="text-[10px]">disabled</Badge>
                    )}
                  </div>
                  {typeof s.target_config?.url === 'string' && (
                    <p className="text-xs text-muted-foreground mt-0.5 truncate max-w-sm">
                      {s.target_config.url as string}
                    </p>
                  )}
                </div>
              </div>
              <Button
                variant="ghost" size="icon" className="w-7 h-7 opacity-0 group-hover:opacity-100 transition-opacity"
                onClick={() => deleteMutation.mutate(s.id)}
              >
                <Trash2 className="w-3.5 h-3.5 text-muted-foreground" />
              </Button>
            </div>
          ))}
        </div>
      )}

      {/* Create dialog */}
      <Dialog open={createOpen} onOpenChange={setCreateOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>New event subscription</DialogTitle>
            <DialogDescription>
              Forward a database event to an external target.
            </DialogDescription>
          </DialogHeader>
          <div className="space-y-3 py-2">
            <div className="space-y-1.5">
              <Label>Event pattern</Label>
              <Input
                placeholder="e.g. users.inserted or orders.*"
                className="font-mono"
                value={form.event_pattern}
                onChange={(e) => setForm((f) => ({ ...f, event_pattern: e.target.value }))}
              />
              <p className="text-[10px] text-muted-foreground">
                Use <code>table.event</code> (insert/update/delete/*)  or <code>*</code> for all events.
              </p>
            </div>
            <div className="space-y-1.5">
              <Label>Target type</Label>
              <Select value={form.target_type} onValueChange={(v) => setForm((f) => ({ ...f, target_type: v }))}>
                <SelectTrigger><SelectValue /></SelectTrigger>
                <SelectContent>
                  <SelectItem value="webhook">Webhook</SelectItem>
                  <SelectItem value="function">Function</SelectItem>
                  <SelectItem value="queue_job">Queue job</SelectItem>
                </SelectContent>
              </Select>
            </div>
            <div className="space-y-1.5">
              <Label>{form.target_type === 'webhook' ? 'Webhook URL' : 'Target URL / ID'}</Label>
              <Input
                placeholder={form.target_type === 'webhook' ? 'https://…' : 'function-id or queue-name'}
                value={form.target_url}
                onChange={(e) => setForm((f) => ({ ...f, target_url: e.target.value }))}
              />
            </div>
          </div>
          <DialogFooter>
            <Button variant="ghost" onClick={() => setCreateOpen(false)}>Cancel</Button>
            <Button
              onClick={() => createMutation.mutate()}
              disabled={!form.event_pattern || !form.target_url || createMutation.isPending}
            >
              {createMutation.isPending ? 'Saving…' : 'Create subscription'}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  )
}
