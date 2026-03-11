'use client'

import { useState } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { Plus, Code2, Trash2, ChevronRight } from 'lucide-react'
import { useParams, useRouter } from 'next/navigation'
import { apiFetch } from '@/lib/api'
import { useStore } from '@/state/tenantStore'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Badge } from '@/components/ui/badge'
import {
  Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription, DialogFooter,
} from '@/components/ui/dialog'

interface Fn { id: string; name: string; runtime: string; created_at: string }

const RUNTIMES = ['deno', 'nodejs', 'python', 'bun']

export default function FunctionsPage() {
  const { projectId } = useParams() as any
  const { projectId: storeId } = useStore()
  const effectiveProjectId = projectId ?? storeId
  const queryClient = useQueryClient()
  const router = useRouter()
  const [createOpen, setCreateOpen] = useState(false)
  const [name, setName] = useState('')
  const [runtime, setRuntime] = useState('deno')

  const { data, isLoading } = useQuery({
    queryKey: ['functions', effectiveProjectId],
    queryFn: () => apiFetch<{ functions: Fn[] }>('/functions'),
    enabled: !!effectiveProjectId,
  })

  const createMutation = useMutation({
    mutationFn: () =>
      apiFetch('/functions', {
        method: 'POST',
        body: JSON.stringify({ name, runtime }),
      }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['functions'] })
      setName('')
      setRuntime('deno')
      setCreateOpen(false)
    },
  })

  const deleteMutation = useMutation({
    mutationFn: (id: string) => apiFetch(`/functions/${id}`, { method: 'DELETE' }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['functions'] }),
  })

  const functions = data?.functions ?? []

  return (
    <div className="p-8 max-w-5xl mx-auto">
      <div className="flex items-center justify-between mb-8">
        <div>
          <h1 className="text-2xl font-bold">Functions</h1>
          <p className="text-sm text-muted-foreground mt-0.5">
            {functions.length} function{functions.length !== 1 ? 's' : ''}
          </p>
        </div>
        <Button onClick={() => setCreateOpen(true)}>
          <Plus className="w-4 h-4" />
          New function
        </Button>
      </div>

      {isLoading ? (
        <div className="space-y-3">
          {[...Array(3)].map((_, i) => (
            <div key={i} className="h-16 rounded-xl bg-muted/40 animate-pulse" />
          ))}
        </div>
      ) : functions.length === 0 ? (
        <div className="rounded-xl border border-dashed p-10 text-center">
          <Code2 className="w-8 h-8 mx-auto mb-3 text-muted-foreground/40" />
          <p className="font-medium mb-1">No functions yet</p>
          <p className="text-sm text-muted-foreground mb-4">Create your first serverless function.</p>
          <Button onClick={() => setCreateOpen(true)}>
            <Plus className="w-4 h-4" /> Create function
          </Button>
        </div>
      ) : (
        <div className="rounded-xl border overflow-hidden">
          {functions.map((fn, i) => (
            <div
              key={fn.id}
              className={`flex items-center gap-4 px-4 py-3.5 hover:bg-muted/30 transition-colors cursor-pointer border-b last:border-0 group ${i === 0 ? '' : ''}`}
              onClick={() => router.push(`/dashboard/projects/${effectiveProjectId}/functions/${fn.id}`)}
            >
              <div className="flex items-center justify-center w-8 h-8 rounded-lg bg-primary/10 shrink-0">
                <Code2 className="w-4 h-4 text-primary" />
              </div>
              <div className="flex-1 min-w-0">
                <p className="font-medium text-sm">{fn.name}</p>
                <p className="text-xs text-muted-foreground font-mono">{fn.id.slice(0, 16)}…</p>
              </div>
              <Badge variant="secondary" className="font-mono text-xs shrink-0">{fn.runtime}</Badge>
              <button
                className="opacity-0 group-hover:opacity-100 transition-opacity p-1.5 rounded hover:bg-destructive/10 text-muted-foreground hover:text-destructive shrink-0"
                onClick={(e) => {
                  e.stopPropagation()
                  if (confirm(`Delete "${fn.name}"?`)) deleteMutation.mutate(fn.id)
                }}
              >
                <Trash2 className="w-3.5 h-3.5" />
              </button>
              <ChevronRight className="w-4 h-4 text-muted-foreground/40 shrink-0" />
            </div>
          ))}
        </div>
      )}

      <Dialog open={createOpen} onOpenChange={setCreateOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Create function</DialogTitle>
            <DialogDescription>Define a new serverless function in this project.</DialogDescription>
          </DialogHeader>
          <div className="space-y-4">
            <div className="space-y-2">
              <Label>Function name</Label>
              <Input
                placeholder="send-email"
                value={name}
                onChange={(e) => setName(e.target.value)}
              />
            </div>
            <div className="space-y-2">
              <Label>Runtime</Label>
              <Select value={runtime} onValueChange={setRuntime}>
                <SelectTrigger>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {RUNTIMES.map((r) => (
                    <SelectItem key={r} value={r}>{r}</SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            {createMutation.isError && (
              <p className="text-sm text-destructive">{createMutation.error.message}</p>
            )}
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setCreateOpen(false)}>Cancel</Button>
            <Button onClick={() => createMutation.mutate()} disabled={!name.trim() || createMutation.isPending}>
              {createMutation.isPending ? 'Creating…' : 'Create function'}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  )
}
