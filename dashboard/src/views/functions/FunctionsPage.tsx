'use client'

import { useState } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { Plus, Code2, Trash2, ChevronRight, Terminal, ArrowRight, Clock } from 'lucide-react'
import { useParams, useRouter } from 'next/navigation'
import { apiFetch } from '@/lib/api'
import type { FunctionResponse } from '@flux/api-types'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Badge } from '@/components/ui/badge'
import { PageHeader } from '@/components/layout/PageHeader'
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription, DialogFooter } from '@/components/ui/dialog'
import { cn } from '@/lib/utils'

const RUNTIMES = ['deno', 'python', 'go', 'java', 'php', 'rust', 'csharp', 'ruby']
const RUNTIME_COLOR: Record<string, string> = {
  deno:   'text-emerald-400 bg-emerald-500/10',
  python: 'text-blue-400   bg-blue-500/10',
  go:     'text-cyan-400   bg-cyan-500/10',
  java:   'text-orange-400 bg-orange-500/10',
  php:    'text-indigo-400 bg-indigo-500/10',
  rust:   'text-amber-400  bg-amber-500/10',
  csharp: 'text-violet-400 bg-violet-500/10',
  ruby:   'text-red-400    bg-red-500/10',
}

function relTime(ts: string) {
  const d = Date.now() - new Date(ts).getTime()
  if (d < 60_000)     return `${Math.floor(d / 1000)}s ago`
  if (d < 3_600_000)  return `${Math.floor(d / 60_000)}m ago`
  if (d < 86_400_000) return `${Math.floor(d / 3_600_000)}h ago`
  return new Date(ts).toLocaleDateString()
}

export default function FunctionsPage() {
  const queryClient = useQueryClient()
  const router = useRouter()
  const [createOpen, setCreateOpen] = useState(false)
  const [name, setName] = useState('')
  const [runtime, setRuntime] = useState('deno')

  const { data, isLoading } = useQuery({
    queryKey: ['functions'],
    queryFn: () => apiFetch<{ functions: FunctionResponse[] }>('/functions'),
  })

  const createMutation = useMutation({
    mutationFn: () => apiFetch('/functions', { method: 'POST', body: JSON.stringify({ name, runtime }) }),
    onSuccess: () => { queryClient.invalidateQueries({ queryKey: ['functions'] }); setName(''); setRuntime('deno'); setCreateOpen(false) },
  })

  const deleteMutation = useMutation({
    mutationFn: (id: string) => apiFetch(`/functions/${id}`, { method: 'DELETE' }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['functions'] }),
  })

  const functions = data?.functions ?? []

  return (
    <div className="flex flex-col h-full">
      <PageHeader
        title="Functions"
        description={functions.length > 0 ? `${functions.length} deployed` : 'Serverless runtime'}
        breadcrumbs={[
          { label: 'Functions' },
        ]}
        actions={
          <Button size="sm" onClick={() => setCreateOpen(true)}>
            <Plus className="w-3.5 h-3.5" /> New function
          </Button>
        }
      />

      <div className="flex-1 overflow-y-auto">
        <div className="p-6 max-w-4xl mx-auto">
          {isLoading ? (
            <div className="rounded-xl border overflow-hidden divide-y divide-border">
              {[...Array(4)].map((_, i) => (
                <div key={i} className="flex items-center gap-4 px-5 py-4">
                  <div className="w-2 h-2 rounded-full bg-muted/40 shrink-0" />
                  <div className="h-4 flex-1 rounded bg-muted/40 animate-pulse" />
                  <div className="h-5 w-14 rounded bg-muted/40 animate-pulse" />
                </div>
              ))}
            </div>
          ) : functions.length === 0 ? (
            <div className="space-y-5">
              <div className="rounded-2xl border border-[#6c63ff]/20 bg-[#6c63ff]/[0.04] p-7">
                <div className="flex items-start gap-5">
                  <div className="w-10 h-10 rounded-xl bg-[#6c63ff]/15 flex items-center justify-center shrink-0">
                    <Code2 className="w-5 h-5 text-[#a78bfa]" />
                  </div>
                  <div className="flex-1">
                    <h2 className="text-base font-semibold mb-1">Deploy your first function</h2>
                    <p className="text-sm text-muted-foreground leading-relaxed mb-5">
                      Functions are sandboxed TypeScript isolates with full access to your project's data engine, secrets, and storage.
                    </p>
                    <div className="flex items-center gap-3 flex-wrap">
                      <Button size="sm" onClick={() => setCreateOpen(true)}><Plus className="w-3.5 h-3.5" /> Create function</Button>
                      <a href="https://fluxbase.co/docs/runtime" target="_blank" className="flex items-center gap-1.5 text-sm text-muted-foreground hover:text-foreground transition-colors">
                        Runtime docs <ArrowRight className="w-3.5 h-3.5" />
                      </a>
                    </div>
                  </div>
                </div>
              </div>
              <div className="grid grid-cols-1 lg:grid-cols-2 gap-4">
                <div className="rounded-xl border p-5 space-y-4">
                  <p className="text-xs font-semibold uppercase tracking-widest text-muted-foreground/40">Workflow</p>
                  {[
                    { n: '1', label: 'Write a function',  desc: 'TypeScript, Python, Go, Java, PHP, Rust, C#, or Ruby.',  cmd: null },
                    { n: '2', label: 'Deploy from CLI',   desc: 'Push to any project in seconds.',              cmd: 'flux deploy' },
                    { n: '3', label: 'Attach a route',    desc: 'Map an HTTP method and path to your function.',cmd: null },
                    { n: '4', label: 'Trace execution',   desc: 'Every request is recorded end-to-end.',        cmd: 'flux why <id>' },
                  ].map((step) => (
                    <div key={step.n} className="flex items-start gap-3">
                      <div className="w-5 h-5 rounded-full bg-[#6c63ff]/15 border border-[#6c63ff]/20 flex items-center justify-center text-[10px] font-bold text-[#a78bfa] shrink-0 mt-0.5">{step.n}</div>
                      <div>
                        <p className="text-sm font-medium">{step.label}</p>
                        <p className="text-xs text-muted-foreground mt-0.5">{step.desc}</p>
                        {step.cmd && <code className="mt-1.5 inline-block text-xs bg-black/30 border border-white/5 px-2.5 py-0.5 rounded font-mono text-emerald-400">{step.cmd}</code>}
                      </div>
                    </div>
                  ))}
                </div>
                <div className="rounded-xl border overflow-hidden">
                  <div className="px-4 py-2 border-b bg-muted/30 flex items-center gap-2">
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
              </div>
            </div>
          ) : (
            <div className="rounded-xl border bg-card overflow-hidden divide-y divide-border/60">
              {functions.map((fn) => (
                <div
                  key={fn.id}
                  className="flex items-center gap-4 px-5 py-3.5 hover:bg-muted/20 transition-colors cursor-pointer group"
                  onClick={() => router.push(`/dashboard/functions/${fn.id}`)}
                >
                  <div className="w-1.5 h-1.5 rounded-full bg-emerald-400 shrink-0" />
                  <div className="w-8 h-8 rounded-lg bg-[#6c63ff]/10 flex items-center justify-center shrink-0">
                    <Code2 className="w-4 h-4 text-[#a78bfa]" />
                  </div>
                  <div className="flex-1 min-w-0">
                    <p className="font-medium text-sm">{fn.name}</p>
                    <p className="text-xs text-muted-foreground font-mono mt-0.5">{fn.id.slice(0, 12)}…</p>
                  </div>
                  <Badge className={cn('font-mono text-[11px] border-0 shrink-0', RUNTIME_COLOR[fn.runtime] ?? 'text-muted-foreground bg-muted/40')}>{fn.runtime}</Badge>
                  <div className="hidden sm:flex items-center gap-1 text-xs text-muted-foreground shrink-0">
                    <Clock className="w-3 h-3" />
                    {relTime(fn.created_at)}
                  </div>
                  <button
                    className="opacity-0 group-hover:opacity-100 transition-opacity p-1.5 rounded hover:bg-destructive/10 text-muted-foreground hover:text-destructive shrink-0"
                    onClick={(e) => { e.stopPropagation(); if (confirm(`Delete "${fn.name}"?`)) deleteMutation.mutate(fn.id) }}
                  >
                    <Trash2 className="w-3.5 h-3.5" />
                  </button>
                  <ChevronRight className="w-4 h-4 text-muted-foreground/30 shrink-0" />
                </div>
              ))}
            </div>
          )}
        </div>
      </div>

      <Dialog open={createOpen} onOpenChange={setCreateOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Create function</DialogTitle>
            <DialogDescription>Define a new serverless function in this project.</DialogDescription>
          </DialogHeader>
          <div className="space-y-4">
            <div className="space-y-2">
              <Label>Function name</Label>
              <Input placeholder="send-email" value={name} onChange={(e) => setName(e.target.value)} />
            </div>
            <div className="space-y-2">
              <Label>Runtime</Label>
              <Select value={runtime} onValueChange={setRuntime}>
                <SelectTrigger><SelectValue /></SelectTrigger>
                <SelectContent>
                  {RUNTIMES.map((r) => <SelectItem key={r} value={r}>{r}</SelectItem>)}
                </SelectContent>
              </Select>
            </div>
            {createMutation.isError && <p className="text-sm text-destructive">{createMutation.error.message}</p>}
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
