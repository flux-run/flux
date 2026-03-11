'use client'

import { useState } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import {
  Plus, FolderOpen, Trash2, ArrowRight, Terminal, Zap,
  Globe, Code2, Database, GitBranch, BookOpen,
} from 'lucide-react'
import Link from 'next/link'
import { useRouter } from 'next/navigation'
import { apiFetch } from '@/lib/api'
import { useStore } from '@/state/tenantStore'
import { useProject } from '@/hooks/useProject'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { cn } from '@/lib/utils'
import {
  Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription, DialogFooter,
} from '@/components/ui/dialog'
import {
  Card, CardContent, CardDescription, CardHeader, CardTitle,
} from '@/components/ui/card'

interface Project { id: string; name: string }

export default function ProjectsPage() {
  const { tenantId } = useStore()
  const { switchProject } = useProject()
  const router = useRouter()
  const queryClient = useQueryClient()
  const [createOpen, setCreateOpen] = useState(false)
  const [newName, setNewName] = useState('')

  const { data, isLoading } = useQuery({
    queryKey: ['projects', tenantId],
    queryFn: () => apiFetch<{ projects: Project[] }>('/projects'),
    enabled: !!tenantId,
  })

  const createMutation = useMutation({
    mutationFn: (name: string) =>
      apiFetch<{ project_id: string }>('/projects', {
        method: 'POST',
        body: JSON.stringify({ name }),
      }),
    onSuccess: (res) => {
      switchProject(res.project_id, newName)
      queryClient.invalidateQueries({ queryKey: ['projects'] })
      setNewName('')
      setCreateOpen(false)
      router.push(`/dashboard/projects/${res.project_id}/overview`)
    },
  })

  const deleteMutation = useMutation({
    mutationFn: (id: string) =>
      apiFetch(`/projects/${id}`, { method: 'DELETE' }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['projects'] }),
  })

  const projects = data?.projects ?? []

  const openProject = (p: Project) => {
    switchProject(p.id, p.name)
    router.push(`/dashboard/projects/${p.id}/overview`)
  }

  return (
    <div className="p-8 max-w-5xl mx-auto">
      {/* Header */}
      <div className="flex items-center justify-between mb-8">
        <div>
          <h1 className="text-2xl font-bold">Projects</h1>
          <p className="text-sm text-muted-foreground mt-0.5">
            {projects.length} project{projects.length !== 1 ? 's' : ''} in this tenant
          </p>
        </div>
        <Button onClick={() => setCreateOpen(true)} disabled={!tenantId}>
          <Plus className="w-4 h-4" />
          New project
        </Button>
      </div>

      {!tenantId && (
        <div className="rounded-xl border border-dashed p-10 text-center text-muted-foreground">
          <FolderOpen className="w-8 h-8 mx-auto mb-3 opacity-30" />
          <p className="text-sm">Select a tenant from the sidebar to view projects.</p>
        </div>
      )}

      {tenantId && isLoading && (
        <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4">
          {[...Array(3)].map((_, i) => (
            <div key={i} className="h-28 rounded-xl bg-muted/40 animate-pulse" />
          ))}
        </div>
      )}

      {tenantId && !isLoading && projects.length === 0 && (
        <div className="space-y-6">
          {/* Welcome hero */}
          <div className="rounded-2xl border border-[#6c63ff]/20 bg-[#6c63ff]/[0.04] p-8">
            <div className="max-w-2xl">
              <div className="inline-flex items-center gap-2 text-xs font-semibold text-[#a78bfa] bg-[#6c63ff]/15 border border-[#6c63ff]/20 rounded-full px-3 py-1 mb-4">
                <Zap className="w-3 h-3" />
                Welcome to Fluxbase
              </div>
              <h2 className="text-2xl font-bold tracking-tight mb-2">
                Build backends where every request is inspectable history
              </h2>
              <p className="text-muted-foreground text-sm leading-relaxed mb-6">
                Deploy TypeScript functions, attach HTTP routes, then trace every execution —
                inputs, outputs, spans, failures — with a single CLI command.
              </p>
              <div className="flex items-center gap-3">
                <Button onClick={() => setCreateOpen(true)}>
                  <Plus className="w-4 h-4" />
                  Create your first project
                </Button>
                <Link
                  href="https://fluxbase.co/docs/quickstart"
                  target="_blank"
                  className="flex items-center gap-1.5 text-sm text-muted-foreground hover:text-foreground transition-colors"
                >
                  <BookOpen className="w-4 h-4" />
                  Quickstart guide
                  <ArrowRight className="w-3.5 h-3.5" />
                </Link>
              </div>
            </div>
          </div>

          <div className="grid grid-cols-1 lg:grid-cols-2 gap-4">
            {/* Get started steps */}
            <Card>
              <CardHeader className="pb-3">
                <CardTitle className="text-sm font-semibold">Get started in 3 minutes</CardTitle>
              </CardHeader>
              <CardContent className="space-y-4">
                {[
                  { n: '1', label: 'Create a project', desc: 'Group your functions, routes, and secrets.', done: false },
                  { n: '2', label: 'Deploy a function', desc: 'Write TypeScript locally, push with the CLI.', cmd: 'flux deploy', done: false },
                  { n: '3', label: 'Create an HTTP route', desc: 'Attach a route to your function.', done: false },
                  { n: '4', label: 'Trace any request', desc: 'Every execution becomes a queryable record.', cmd: 'flux why <request-id>', done: false },
                ].map((step) => (
                  <div key={step.n} className="flex items-start gap-3">
                    <div className="w-6 h-6 rounded-full bg-[#6c63ff]/15 border border-[#6c63ff]/20 flex items-center justify-center text-xs font-bold text-[#a78bfa] shrink-0 mt-0.5">
                      {step.n}
                    </div>
                    <div className="flex-1 min-w-0">
                      <p className="text-sm font-medium leading-tight">{step.label}</p>
                      <p className="text-xs text-muted-foreground mt-0.5">{step.desc}</p>
                      {step.cmd && (
                        <code className="mt-1.5 inline-block text-xs bg-black/30 border border-white/5 px-2.5 py-1 rounded-lg font-mono text-emerald-400">
                          {step.cmd}
                        </code>
                      )}
                    </div>
                  </div>
                ))}
              </CardContent>
            </Card>

            {/* Architecture + CLI */}
            <div className="space-y-4">
              {/* Execution flow */}
              <Card>
                <CardHeader className="pb-3">
                  <CardTitle className="text-sm font-semibold">How a request flows</CardTitle>
                </CardHeader>
                <CardContent>
                  <div className="space-y-1">
                    {[
                      { icon: Globe,      label: 'Gateway',      sub: 'HTTP entry point',   color: 'text-[#a78bfa]',  bg: 'bg-[#6c63ff]/10' },
                      { icon: Code2,      label: 'Function',     sub: 'Your TypeScript',    color: 'text-emerald-400', bg: 'bg-emerald-500/10' },
                      { icon: Database,   label: 'Data Engine',  sub: 'Postgres + files',   color: 'text-blue-400',   bg: 'bg-blue-500/10' },
                      { icon: GitBranch,  label: 'Trace',        sub: 'Recorded forever',   color: 'text-amber-400',  bg: 'bg-amber-500/10' },
                    ].map((node, i, arr) => (
                      <div key={node.label} className="flex items-center gap-3">
                        <div className={cn('w-7 h-7 rounded-lg flex items-center justify-center shrink-0', node.bg)}>
                          <node.icon className={cn('w-3.5 h-3.5', node.color)} />
                        </div>
                        <div className="flex-1 min-w-0">
                          <p className="text-xs font-medium leading-tight">{node.label}</p>
                          <p className="text-[10px] text-muted-foreground">{node.sub}</p>
                        </div>
                        {i < arr.length - 1 && (
                          <div className="w-px h-3 bg-white/10 absolute" style={{ display: 'none' }} />
                        )}
                      </div>
                    ))}
                  </div>
                  <div className="mt-3 border-t border-white/5 pt-3">
                    <p className="text-[10px] text-muted-foreground/60 leading-relaxed">
                      Execution becomes inspectable history. Trace, replay, and debug any request.
                    </p>
                  </div>
                </CardContent>
              </Card>

              {/* CLI quickstart */}
              <Card>
                <CardHeader className="pb-3">
                  <div className="flex items-center gap-2">
                    <Terminal className="w-3.5 h-3.5 text-muted-foreground" />
                    <CardTitle className="text-sm font-semibold">Install the CLI</CardTitle>
                  </div>
                </CardHeader>
                <CardContent className="space-y-2">
                  <div className="rounded-lg bg-black/30 border border-white/5 px-3 py-2.5 font-mono text-xs">
                    <p className="text-muted-foreground/50 text-[10px] mb-1"># install</p>
                    <p className="text-emerald-400">curl -fsSL https://fluxbase.co/install | bash</p>
                  </div>
                  <div className="rounded-lg bg-black/30 border border-white/5 px-3 py-2.5 font-mono text-xs space-y-0.5">
                    <p className="text-muted-foreground/50 text-[10px] mb-1"># deploy &amp; trace</p>
                    <p className="text-[#a78bfa]">flux login</p>
                    <p className="text-[#a78bfa]">flux deploy</p>
                    <p className="text-[#a78bfa]">flux why &lt;request-id&gt;</p>
                  </div>
                </CardContent>
              </Card>
            </div>
          </div>

          {/* Template ideas */}
          <Card>
            <CardHeader className="pb-3">
              <CardTitle className="text-sm font-semibold">Start from a template</CardTitle>
              <CardDescription>Common project types to get you started</CardDescription>
            </CardHeader>
            <CardContent className="grid grid-cols-2 lg:grid-cols-4 gap-3">
              {[
                { label: 'REST API',              desc: 'CRUD + auth + routes' },
                { label: 'AI Agent',              desc: 'Tool calls + trace replay' },
                { label: 'Event-driven workflow', desc: 'Async queues + chains' },
                { label: 'Background jobs',       desc: 'Cron + retry + audit' },
              ].map((t) => (
                <button
                  key={t.label}
                  onClick={() => setCreateOpen(true)}
                  className="text-left rounded-xl border border-white/8 bg-white/[0.02] hover:bg-white/[0.05] hover:border-[#6c63ff]/30 p-4 transition-all group"
                >
                  <p className="text-sm font-medium mb-1 group-hover:text-[#a78bfa] transition-colors">{t.label}</p>
                  <p className="text-xs text-muted-foreground/60">{t.desc}</p>
                </button>
              ))}
            </CardContent>
          </Card>
        </div>
      )}

      {!isLoading && projects.length > 0 && (
        <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4">
          {projects.map((p) => (
            <Card
              key={p.id}
              className="group cursor-pointer hover:border-primary/40 hover:shadow-md hover:shadow-primary/5 transition-all duration-200"
              onClick={() => openProject(p)}
            >
              <CardHeader className="pb-2">
                <div className="flex items-start justify-between">
                  <div className="flex items-center justify-center w-9 h-9 rounded-lg bg-primary/10 mb-2">
                    <FolderOpen className="w-4 h-4 text-primary" />
                  </div>
                  <button
                    className="opacity-0 group-hover:opacity-100 transition-opacity p-1 rounded hover:bg-destructive/10 text-muted-foreground hover:text-destructive"
                    onClick={(e) => {
                      e.stopPropagation()
                      if (confirm(`Delete project "${p.name}"?`)) deleteMutation.mutate(p.id)
                    }}
                  >
                    <Trash2 className="w-3.5 h-3.5" />
                  </button>
                </div>
                <CardTitle className="text-base">{p.name}</CardTitle>
                <CardDescription className="font-mono text-xs truncate">{p.id}</CardDescription>
              </CardHeader>
              <CardContent>
                <div className="flex items-center text-xs text-primary font-medium gap-1 group-hover:gap-2 transition-all">
                  Open project <ArrowRight className="w-3 h-3" />
                </div>
              </CardContent>
            </Card>
          ))}
        </div>
      )}

      {/* Create Dialog */}
      <Dialog open={createOpen} onOpenChange={setCreateOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Create project</DialogTitle>
            <DialogDescription>
              Projects group your functions, secrets, and API keys.
            </DialogDescription>
          </DialogHeader>
          <div className="space-y-3">
            <Label htmlFor="proj-name">Project name</Label>
            <Input
              id="proj-name"
              placeholder="backend"
              value={newName}
              onChange={(e) => setNewName(e.target.value)}
              onKeyDown={(e) => e.key === 'Enter' && newName.trim() && createMutation.mutate(newName.trim())}
            />
            {createMutation.isError && (
              <p className="text-sm text-destructive">{createMutation.error.message}</p>
            )}
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setCreateOpen(false)}>Cancel</Button>
            <Button
              onClick={() => createMutation.mutate(newName.trim())}
              disabled={!newName.trim() || createMutation.isPending}
            >
              {createMutation.isPending ? 'Creating…' : 'Create project'}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  )
}
