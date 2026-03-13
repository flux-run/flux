'use client'

import { useMemo, useState } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { useParams } from 'next/navigation'
import { Webhook, Plus, Trash2, AlertCircle } from 'lucide-react'
import { apiFetch } from '@/lib/api'
import { Button } from '@/components/ui/button'
import { Label } from '@/components/ui/label'
import {
  Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription, DialogFooter,
} from '@/components/ui/dialog'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'

interface Hook {
  id: string
  table_name: string
  event: string
  function_id: string
  enabled: boolean
}

interface HookResponse { hooks: Hook[] }
interface Fn { id: string; name: string }
interface FnResponse { functions: Fn[] }

interface Props { table: string }

const EVENT_COLORS: Record<string, string> = {
  before_insert: 'bg-sky-500/10 text-sky-700 dark:text-sky-400',
  after_insert:  'bg-emerald-500/10 text-emerald-700 dark:text-emerald-400',
  before_update: 'bg-amber-500/10 text-amber-700 dark:text-amber-400',
  after_update:  'bg-orange-500/10 text-orange-700 dark:text-orange-400',
  before_delete: 'bg-red-500/10 text-red-700 dark:text-red-400',
  after_delete:  'bg-pink-500/10 text-pink-700 dark:text-pink-400',
}

const HOOK_EVENTS = Object.keys(EVENT_COLORS)

export default function TableHooksView({ table }: Props) {
  const { projectId } = useParams() as any
  const queryClient = useQueryClient()
  const [createOpen, setCreateOpen] = useState(false)
  const [event, setEvent] = useState('after_insert')
  const [fnId, setFnId] = useState('')

  const hooksQ = useQuery({
    queryKey: ['hooks', projectId],
    queryFn: () => apiFetch<HookResponse>('/db/hooks'),
    enabled: !!projectId,
  })

  const fnsQ = useQuery({
    queryKey: ['functions', projectId],
    queryFn: () => apiFetch<FnResponse>('/functions'),
    enabled: !!projectId,
  })

  const hooks = useMemo(
    () => (hooksQ.data?.hooks ?? []).filter((h) => h.table_name === table),
    [hooksQ.data, table],
  )

  const fnMap = useMemo(() => {
    const m: Record<string, string> = {}
    for (const f of fnsQ.data?.functions ?? []) m[f.id] = f.name
    return m
  }, [fnsQ.data])

  const deleteMutation = useMutation({
    mutationFn: (id: string) => apiFetch(`/db/hooks/${id}`, { method: 'DELETE' }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['hooks'] }),
  })

  const toggleMutation = useMutation({
    mutationFn: ({ id, enabled }: { id: string; enabled: boolean }) =>
      apiFetch(`/db/hooks/${id}`, {
        method: 'PATCH',
        body: JSON.stringify({ enabled }),
      }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['hooks'] }),
  })

  const createMutation = useMutation({
    mutationFn: () =>
      apiFetch('/db/hooks', {
        method: 'POST',
        body: JSON.stringify({ table_name: table, event, function_id: fnId }),
      }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['hooks'] })
      setCreateOpen(false)
    },
  })

  if (hooksQ.error) {
    return (
      <div className="flex flex-col items-center justify-center py-20 gap-2 text-muted-foreground">
        <AlertCircle className="w-6 h-6 text-destructive" />
        <p className="text-sm">{String((hooksQ.error as Error).message)}</p>
      </div>
    )
  }

  return (
    <div className="p-6 max-w-4xl mx-auto">
      <div className="flex items-center justify-between mb-6">
        <div>
          <h2 className="text-sm font-semibold text-muted-foreground uppercase tracking-widest">
            Hooks — {table}
          </h2>
          <p className="text-xs text-muted-foreground mt-0.5">
            Functions triggered on table mutations
          </p>
        </div>
        <Button size="sm" onClick={() => setCreateOpen(true)}>
          <Plus className="w-3.5 h-3.5 mr-1.5" /> Add hook
        </Button>
      </div>

      {hooksQ.isLoading ? (
        <div className="space-y-2">
          {[...Array(3)].map((_, i) => (
            <div key={i} className="h-14 rounded-xl border bg-card animate-pulse" />
          ))}
        </div>
      ) : hooks.length === 0 ? (
        <div className="flex flex-col items-center justify-center py-16 text-center border rounded-xl bg-card gap-2">
          <Webhook className="w-8 h-8 text-muted-foreground/40" />
          <p className="text-sm font-medium">No hooks on {table}</p>
          <p className="text-xs text-muted-foreground">
            Hooks run a serverless function before or after a table mutation.
          </p>
          <Button size="sm" variant="outline" className="mt-2" onClick={() => setCreateOpen(true)}>
            <Plus className="w-3.5 h-3.5 mr-1.5" /> Add first hook
          </Button>
        </div>
      ) : (
        <div className="rounded-xl border divide-y overflow-hidden bg-card">
          {hooks.map((h) => (
            <div key={h.id} className="group flex items-center justify-between px-5 py-3.5 hover:bg-muted/20 transition-colors">
              <div className="flex items-center gap-3">
                <span className={`inline-flex text-[10px] font-semibold px-2 py-0.5 rounded-full ${EVENT_COLORS[h.event] ?? 'bg-muted'}`}>
                  {h.event}
                </span>
                <div>
                  <p className="text-sm font-medium">{fnMap[h.function_id] ?? h.function_id}</p>
                  <p className="text-[10px] text-muted-foreground font-mono">{h.function_id}</p>
                </div>
              </div>
              <div className="flex items-center gap-2">
                <button
                  onClick={() => toggleMutation.mutate({ id: h.id, enabled: !h.enabled })}
                  className={`relative inline-flex h-4 w-8 items-center rounded-full transition-colors ${h.enabled ? 'bg-primary' : 'bg-muted'}`}
                >
                  <span className={`inline-block h-3 w-3 rounded-full bg-white transition-transform ${h.enabled ? 'translate-x-4.5' : 'translate-x-0.5'}`} />
                </button>
                <Button
                  variant="ghost" size="icon" className="w-7 h-7 opacity-0 group-hover:opacity-100 transition-opacity"
                  onClick={() => deleteMutation.mutate(h.id)}
                >
                  <Trash2 className="w-3.5 h-3.5 text-muted-foreground" />
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
            <DialogTitle>Add hook</DialogTitle>
            <DialogDescription>
              Trigger a function when a mutation happens on <strong>{table}</strong>.
            </DialogDescription>
          </DialogHeader>
          <div className="space-y-3 py-2">
            <div className="space-y-1.5">
              <Label>Event</Label>
              <Select value={event} onValueChange={setEvent}>
                <SelectTrigger><SelectValue /></SelectTrigger>
                <SelectContent>
                  {HOOK_EVENTS.map((e) => (
                    <SelectItem key={e} value={e}>{e}</SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <div className="space-y-1.5">
              <Label>Function</Label>
              <Select value={fnId} onValueChange={setFnId}>
                <SelectTrigger>
                  <SelectValue placeholder="Select a function" />
                </SelectTrigger>
                <SelectContent>
                  {(fnsQ.data?.functions ?? []).map((f) => (
                    <SelectItem key={f.id} value={f.id}>{f.name}</SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
          </div>
          <DialogFooter>
            <Button variant="ghost" onClick={() => setCreateOpen(false)}>Cancel</Button>
            <Button onClick={() => createMutation.mutate()} disabled={!fnId || createMutation.isPending}>
              {createMutation.isPending ? 'Saving…' : 'Add hook'}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  )
}
