import { useState } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { useParams } from 'react-router-dom'
import { GitBranch, Plus, Trash2, ChevronDown, ChevronUp, Zap } from 'lucide-react'
import { apiFetch } from '@/lib/api'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Badge } from '@/components/ui/badge'
import {
  Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription, DialogFooter,
} from '@/components/ui/dialog'

interface WorkflowStep {
  id: string
  step_order: number
  name: string
  action_type: string
  action_config: Record<string, unknown>
}

interface Workflow {
  id: string
  name: string
  description: string | null
  trigger_event: string
  enabled: boolean
  steps: WorkflowStep[] | null
}

interface WfResponse { workflows: Workflow[] }

const ACTION_COLOR: Record<string, string> = {
  function:  'bg-purple-500/10 text-purple-700 dark:text-purple-400',
  queue_job: 'bg-amber-500/10 text-amber-700 dark:text-amber-400',
  webhook:   'bg-sky-500/10 text-sky-700 dark:text-sky-400',
}

export default function WorkflowsPage() {
  const { projectId } = useParams<{ projectId: string }>()
  const queryClient = useQueryClient()
  const [createOpen, setCreateOpen] = useState(false)
  const [expandedId, setExpandedId] = useState<string | null>(null)
  const [form, setForm] = useState({ name: '', description: '', trigger_event: '' })

  const { data, isLoading } = useQuery({
    queryKey: ['workflows', projectId],
    queryFn: () => apiFetch<WfResponse>('/db/workflows'),
    enabled: !!projectId,
  })

  const deleteMutation = useMutation({
    mutationFn: (id: string) => apiFetch(`/db/workflows/${id}`, { method: 'DELETE' }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['workflows'] }),
  })

  const createMutation = useMutation({
    mutationFn: () =>
      apiFetch('/db/workflows', {
        method: 'POST',
        body: JSON.stringify(form),
      }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['workflows'] })
      setCreateOpen(false)
      setForm({ name: '', description: '', trigger_event: '' })
    },
  })

  const workflows = data?.workflows ?? []

  return (
    <div className="p-8 max-w-5xl mx-auto">
      <div className="flex items-center justify-between mb-8">
        <div>
          <h1 className="text-2xl font-bold">Workflows</h1>
          <p className="text-sm text-muted-foreground mt-0.5">
            Event-driven multi-step automations
          </p>
        </div>
        <Button size="sm" onClick={() => setCreateOpen(true)}>
          <Plus className="w-4 h-4 mr-1.5" /> New workflow
        </Button>
      </div>

      {isLoading ? (
        <div className="space-y-2">
          {[...Array(3)].map((_, i) => (
            <div key={i} className="h-16 rounded-xl border bg-card animate-pulse" />
          ))}
        </div>
      ) : workflows.length === 0 ? (
        <div className="flex flex-col items-center justify-center py-20 text-center border rounded-xl bg-card">
          <GitBranch className="w-10 h-10 text-muted-foreground/40 mb-3" />
          <p className="font-medium text-sm">No workflows yet</p>
          <p className="text-xs text-muted-foreground mt-1 mb-4">
            Workflows run a sequence of steps in response to a database event.
          </p>
          <Button size="sm" variant="outline" onClick={() => setCreateOpen(true)}>
            <Plus className="w-4 h-4 mr-1.5" /> Create workflow
          </Button>
        </div>
      ) : (
        <div className="space-y-2">
          {workflows.map((wf) => {
            const steps = wf.steps ?? []
            const expanded = expandedId === wf.id
            return (
              <div key={wf.id} className="rounded-xl border bg-card overflow-hidden">
                <div
                  className="group flex items-center justify-between px-5 py-4 cursor-pointer hover:bg-muted/20 transition-colors"
                  onClick={() => setExpandedId(expanded ? null : wf.id)}
                >
                  <div className="flex items-center gap-3">
                    <GitBranch className="w-4 h-4 text-muted-foreground shrink-0" />
                    <div>
                      <div className="flex items-center gap-2">
                        <p className="font-medium text-sm">{wf.name}</p>
                        {!wf.enabled && (
                          <Badge variant="secondary" className="text-[10px]">disabled</Badge>
                        )}
                      </div>
                      <p className="text-[11px] text-muted-foreground mt-0.5 flex items-center gap-1">
                        <Zap className="w-2.5 h-2.5" /> {wf.trigger_event}
                        <span className="mx-1 text-muted-foreground/40">·</span>
                        {steps.length} step{steps.length !== 1 ? 's' : ''}
                      </p>
                    </div>
                  </div>
                  <div className="flex items-center gap-2">
                    <Button
                      variant="ghost" size="icon" className="w-7 h-7 opacity-0 group-hover:opacity-100 transition-opacity"
                      onClick={(e) => { e.stopPropagation(); deleteMutation.mutate(wf.id) }}
                    >
                      <Trash2 className="w-3.5 h-3.5 text-muted-foreground" />
                    </Button>
                    {expanded ? (
                      <ChevronUp className="w-4 h-4 text-muted-foreground" />
                    ) : (
                      <ChevronDown className="w-4 h-4 text-muted-foreground" />
                    )}
                  </div>
                </div>

                {/* Steps */}
                {expanded && steps.length > 0 && (
                  <div className="border-t bg-muted/10 px-5 py-3 space-y-2">
                    {steps.map((step, i) => (
                      <div key={step.id} className="flex items-start gap-3">
                        <div className="flex flex-col items-center">
                          <div className="w-5 h-5 rounded-full bg-primary/20 text-primary text-[10px] font-bold flex items-center justify-center">
                            {i + 1}
                          </div>
                          {i < steps.length - 1 && (
                            <div className="w-px h-4 bg-border mt-1" />
                          )}
                        </div>
                        <div>
                          <div className="flex items-center gap-2">
                            <p className="text-sm font-medium">{step.name}</p>
                            <span className={`text-[10px] font-semibold px-2 py-0.5 rounded-full ${ACTION_COLOR[step.action_type] ?? 'bg-muted'}`}>
                              {step.action_type}
                            </span>
                          </div>
                        </div>
                      </div>
                    ))}
                  </div>
                )}
              </div>
            )
          })}
        </div>
      )}

      {/* Create dialog */}
      <Dialog open={createOpen} onOpenChange={setCreateOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>New workflow</DialogTitle>
            <DialogDescription>Define a new event-driven automation.</DialogDescription>
          </DialogHeader>
          <div className="space-y-3 py-2">
            <div className="space-y-1.5">
              <Label>Name</Label>
              <Input
                placeholder="e.g. send_welcome_email"
                value={form.name}
                onChange={(e) => setForm((f) => ({ ...f, name: e.target.value }))}
              />
            </div>
            <div className="space-y-1.5">
              <Label>Trigger event</Label>
              <Input
                placeholder="e.g. users.inserted"
                className="font-mono"
                value={form.trigger_event}
                onChange={(e) => setForm((f) => ({ ...f, trigger_event: e.target.value }))}
              />
            </div>
            <div className="space-y-1.5">
              <Label>Description (optional)</Label>
              <Input
                value={form.description}
                onChange={(e) => setForm((f) => ({ ...f, description: e.target.value }))}
              />
            </div>
          </div>
          <DialogFooter>
            <Button variant="ghost" onClick={() => setCreateOpen(false)}>Cancel</Button>
            <Button
              onClick={() => createMutation.mutate()}
              disabled={!form.name || !form.trigger_event || createMutation.isPending}
            >
              {createMutation.isPending ? 'Creating…' : 'Create workflow'}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  )
}
