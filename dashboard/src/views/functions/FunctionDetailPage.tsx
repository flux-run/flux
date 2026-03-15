'use client'

import { useState } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { useParams } from 'next/navigation'
import { Upload, CheckCircle2, Circle, ArrowLeft, Code2, Clock, Layers2 } from 'lucide-react'
import Link from 'next/link'
import { apiFetch } from '@/lib/api'
import type { FunctionResponse, DeploymentResponse } from '@flux/api-types'
import { useStore } from '@/state/tenantStore'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { PageHeader } from '@/components/layout/PageHeader'
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription, DialogFooter } from '@/components/ui/dialog'
import { cn } from '@/lib/utils'

const RUNTIME_COLOR: Record<string, string> = {
  deno:   'text-emerald-400 bg-emerald-500/10 border-emerald-500/20',
  python: 'text-blue-400    bg-blue-500/10    border-blue-500/20',
  go:     'text-cyan-400    bg-cyan-500/10    border-cyan-500/20',
  java:   'text-orange-400  bg-orange-500/10  border-orange-500/20',
  php:    'text-indigo-400  bg-indigo-500/10  border-indigo-500/20',
  rust:   'text-amber-400   bg-amber-500/10   border-amber-500/20',
  csharp: 'text-violet-400  bg-violet-500/10  border-violet-500/20',
  ruby:   'text-red-400     bg-red-500/10     border-red-500/20',
}

function relTime(ts: string) {
  const d = Date.now() - new Date(ts).getTime()
  if (d < 60_000)     return `${Math.floor(d / 1000)}s ago`
  if (d < 3_600_000)  return `${Math.floor(d / 60_000)}m ago`
  if (d < 86_400_000) return `${Math.floor(d / 3_600_000)}h ago`
  return new Date(ts).toLocaleString()
}

export default function FunctionDetailPage() {
  const { projectId, functionId } = useParams() as any
  const { projectName } = useStore()
  const queryClient = useQueryClient()
  const [uploadOpen, setUploadOpen] = useState(false)
  const [storageKey, setStorageKey] = useState('')

  const fnQuery = useQuery({
    queryKey: ['function', functionId],
    queryFn: () => apiFetch<FunctionResponse>(`/functions/${functionId}`),
    enabled: !!functionId,
  })
  const depQuery = useQuery({
    queryKey: ['deployments', functionId],
    queryFn: () => apiFetch<{ deployments: DeploymentResponse[] }>(`/functions/${functionId}/deployments`),
    enabled: !!functionId,
  })

  const uploadMutation = useMutation({
    mutationFn: () => apiFetch(`/functions/${functionId}/deployments`, { method: 'POST', body: JSON.stringify({ storage_key: storageKey }) }),
    onSuccess: () => { queryClient.invalidateQueries({ queryKey: ['deployments', functionId] }); setStorageKey(''); setUploadOpen(false) },
  })

  const fn = fnQuery.data
  const deployments = depQuery.data?.deployments ?? []

  return (
    <div className="flex flex-col h-full">
      <PageHeader
        title={fn?.name ?? '…'}
        breadcrumbs={[
          { label: 'Projects', href: '/dashboard' },
          { label: projectName ?? projectId, href: `/dashboard/projects/${projectId}/overview` },
          { label: 'Functions', href: `/dashboard/projects/${projectId}/functions` },
          { label: fn?.name ?? '…' },
        ]}
        badge={fn && (
          <Badge className={cn('text-[11px] font-mono border', RUNTIME_COLOR[fn.runtime] ?? '')}>{fn.runtime}</Badge>
        )}
        actions={
          <Button size="sm" onClick={() => setUploadOpen(true)}>
            <Upload className="w-3.5 h-3.5" /> Deploy
          </Button>
        }
      />

      <div className="flex-1 overflow-y-auto">
        <div className="p-6 max-w-3xl mx-auto space-y-5">

          {/* Identity card */}
          <div className="rounded-xl border bg-card p-5 grid sm:grid-cols-3 gap-4">
            <div>
              <p className="text-xs text-muted-foreground mb-1">Function ID</p>
              <code className="text-xs font-mono text-foreground/80">{functionId}</code>
            </div>
            <div>
              <p className="text-xs text-muted-foreground mb-1">Runtime</p>
              <p className="text-sm">{fn?.runtime ?? '—'}</p>
            </div>
            <div>
              <p className="text-xs text-muted-foreground mb-1">Deployments</p>
              <p className="text-sm">{deployments.length}</p>
            </div>
          </div>

          {/* Deployments */}
          <div className="rounded-xl border bg-card overflow-hidden">
            <div className="flex items-center justify-between px-5 py-4 border-b">
              <div className="flex items-center gap-2">
                <Layers2 className="w-4 h-4 text-muted-foreground" />
                <span className="text-sm font-semibold">Deployments</span>
              </div>
              <Button size="sm" variant="outline" onClick={() => setUploadOpen(true)}>
                <Upload className="w-3.5 h-3.5" /> Upload
              </Button>
            </div>

            {depQuery.isLoading ? (
              <div className="p-5 space-y-3">
                {[...Array(2)].map((_, i) => <div key={i} className="h-12 rounded-lg bg-muted/40 animate-pulse" />)}
              </div>
            ) : deployments.length === 0 ? (
              <div className="flex flex-col items-center justify-center py-12 text-center px-6">
                <div className="w-10 h-10 rounded-xl bg-muted/40 flex items-center justify-center mb-3">
                  <Code2 className="w-5 h-5 text-muted-foreground/40" />
                </div>
                <p className="text-sm font-medium mb-1">No deployments yet</p>
                <p className="text-xs text-muted-foreground mb-4">Use the CLI to deploy your first bundle.</p>
                <code className="text-xs bg-muted/60 border px-3 py-1.5 rounded-lg font-mono text-emerald-400">flux deploy</code>
              </div>
            ) : (
              <div className="divide-y divide-border/60">
                {deployments.map((d) => (
                  <div key={d.id} className="flex items-center gap-4 px-5 py-3.5">
                    {d.is_active
                      ? <CheckCircle2 className="w-4 h-4 text-emerald-400 shrink-0" />
                      : <Circle className="w-4 h-4 text-muted-foreground/30 shrink-0" />
                    }
                    <div className="flex-1">
                      <div className="flex items-center gap-2">
                        <span className="text-sm font-medium">v{d.version}</span>
                        {d.is_active && <Badge className="text-[10px] font-mono bg-emerald-500/10 text-emerald-400 border-emerald-500/20">active</Badge>}
                      </div>
                      <code className="text-[11px] font-mono text-muted-foreground/50">{d.id.slice(0, 16)}…</code>
                    </div>
                    <div className="flex items-center gap-1.5 text-xs text-muted-foreground shrink-0">
                      <Clock className="w-3 h-3" />
                      {relTime(d.created_at)}
                    </div>
                  </div>
                ))}
              </div>
            )}
          </div>
        </div>
      </div>

      <Dialog open={uploadOpen} onOpenChange={setUploadOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Upload deployment</DialogTitle>
            <DialogDescription>Provide a storage key pointing to the compiled function bundle.</DialogDescription>
          </DialogHeader>
          <div className="space-y-2">
            <Label>Storage key</Label>
            <Input className="font-mono" placeholder="bundles/my-fn-v2.js" value={storageKey} onChange={(e) => setStorageKey(e.target.value)} />
          </div>
          {uploadMutation.isError && <p className="text-sm text-destructive">{uploadMutation.error.message}</p>}
          <DialogFooter>
            <Button variant="outline" onClick={() => setUploadOpen(false)}>Cancel</Button>
            <Button onClick={() => uploadMutation.mutate()} disabled={!storageKey.trim() || uploadMutation.isPending}>
              {uploadMutation.isPending ? 'Deploying…' : 'Deploy'}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  )
}
