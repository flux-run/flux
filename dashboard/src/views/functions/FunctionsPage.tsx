'use client'

import { useState } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { Plus, Code2, Trash2, ChevronRight, Terminal, ArrowRight } from 'lucide-react'
import { useParams, useRouter } from 'next/navigation'
import { apiFetch } from '@/lib/api'
import { useStore } from '@/state/tenantStore'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Badge } from '@/components/ui/badge'
import { cn } from '@/lib/utils'
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
        <div className="space-y-5">
          {/* Hero */}
          <div className="rounded-2xl border border-[#6c63ff]/20 bg-[#6c63ff]/[0.04] p-7">
            <div className="flex items-start gap-5">
              <div className="w-10 h-10 rounded-xl bg-[#6c63ff]/15 flex items-center justify-center shrink-0 mt-0.5">
                <Code2 className="w-5 h-5 text-[#a78bfa]" />
              </div>
              <div className="flex-1 min-w-0">
                <h2 className="text-base font-semibold mb-1">Deploy your first function</h2>
                <p className="text-sm text-muted-foreground leading-relaxed mb-5">
                  Functions are the core runtime primitive. Write TypeScript locally,
                  deploy with the CLI, and every execution is automatically traced.
                </p>
                <div className="flex items-center gap-3 flex-wrap">
                  <Button onClick={() => setCreateOpen(true)}>
                    <Plus className="w-4 h-4" /> Create function
                  </Button>
                  <a
                    href="https://fluxbase.co/docs/runtime"
                    target="_blank"
                    className="flex items-center gap-1.5 text-sm text-muted-foreground hover:text-foreground transition-colors"
                  >
                    Runtime docs <ArrowRight className="w-3.5 h-3.5" />
                  </a>
                </div>
              </div>
            </div>
          </div>

          <div className="grid grid-cols-1 lg:grid-cols-2 gap-4">
            {/* Workflow */}
            <div className="rounded-xl border border-white/8 p-5 space-y-4">
              <p className="text-xs font-semibold uppercase tracking-widest text-muted-foreground/40">Workflow</p>
              {[
                { n: '1', label: 'Write a function', desc: 'TypeScript runs inside sandboxed V8 isolates.', cmd: null },
                { n: '2', label: 'Deploy from CLI',   desc: 'Push to any project in seconds.',             cmd: 'flux deploy' },
                { n: '3', label: 'Attach a route',    desc: 'Map an HTTP method and path to your function.', cmd: null },
                { n: '4', label: 'Trace execution',   desc: 'Every request is recorded end-to-end.',       cmd: 'flux why <id>' },
              ].map((step) => (
                <div key={step.n} className="flex items-start gap-3">
                  <div className="w-5 h-5 rounded-full bg-[#6c63ff]/15 border border-[#6c63ff]/20 flex items-center justify-center text-[10px] font-bold text-[#a78bfa] shrink-0 mt-0.5">
                    {step.n}
                  </div>
                  <div className="flex-1 min-w-0">
                    <p className="text-sm font-medium leading-tight">{step.label}</p>
                    <p className="text-xs text-muted-foreground mt-0.5">{step.desc}</p>
                    {step.cmd && (
                      <code className="mt-1.5 inline-block text-xs bg-black/30 border border-white/5 px-2.5 py-0.5 rounded font-mono text-emerald-400">
                        {step.cmd}
                      </code>
                    )}
                  </div>
                </div>
              ))}
            </div>

            {/* Code + CLI */}
            <div className="space-y-3">
              <div className="rounded-xl border border-white/8 overflow-hidden">
                <div className="px-4 py-2 border-b border-white/5 bg-white/[0.03] flex items-center gap-2">
                  <Code2 className="w-3 h-3 text-muted-foreground/50" />
                  <span className="text-[10px] text-muted-foreground/50 font-mono">functions/hello.ts</span>
                </div>
                <pre className="px-4 py-3 text-xs font-mono text-muted-foreground/80 leading-relaxed overflow-x-auto">{`export default defineFunction({
  name: "hello_world",
  handler: async ({ ctx, input }) => {
    const user = await ctx.db.users.findOne({
      where: { id: input.userId },
    })
    return { message: \`Hello, \${user.name}\` }
  },
})`}</pre>
              </div>
              <div className="rounded-xl border border-white/8 overflow-hidden">
                <div className="px-4 py-2 border-b border-white/5 bg-white/[0.03] flex items-center gap-2">
                  <Terminal className="w-3 h-3 text-muted-foreground/50" />
                  <span className="text-[10px] text-muted-foreground/50 font-mono">terminal</span>
                </div>
                <div className="px-4 py-3 text-xs font-mono space-y-1">
                  <p><span className="text-muted-foreground/30">$</span> <span className="text-emerald-400">flux deploy</span></p>
                  <p className="text-muted-foreground/50">✓ Deployed hello_world (deno · 12ms)</p>
                  <p><span className="text-muted-foreground/30">$</span> <span className="text-emerald-400">flux tail</span></p>
                  <p className="text-[#a78bfa]">→ Watching executions…</p>
                </div>
              </div>
              <div className="rounded-lg border border-white/5 bg-white/[0.02] p-3">
                <p className="text-[10px] text-muted-foreground/50 mb-1">Runtime</p>
                <p className="text-xs text-muted-foreground/70 leading-relaxed">
                  TypeScript functions run in sandboxed V8 isolates with access to
                  your project's data engine, secrets, and storage.
                </p>
              </div>
            </div>
          </div>
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
