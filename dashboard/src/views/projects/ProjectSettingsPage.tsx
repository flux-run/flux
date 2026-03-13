'use client'

import { useState } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { Settings, Copy, Check, Trash2, RefreshCw } from 'lucide-react'
import { useParams, useRouter } from 'next/navigation'
import { apiFetch } from '@/lib/api'
import { useStore } from '@/state/tenantStore'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Separator } from '@/components/ui/separator'
import { PageHeader } from '@/components/layout/PageHeader'

function CopyField({ label, value }: { label: string; value: string }) {
  const [copied, setCopied] = useState(false)
  const copy = () => {
    navigator.clipboard.writeText(value)
    setCopied(true)
    setTimeout(() => setCopied(false), 1500)
  }
  return (
    <div>
      <p className="text-xs text-muted-foreground mb-1.5">{label}</p>
      <div className="flex items-center gap-2">
        <code className="flex-1 text-xs font-mono bg-muted/50 border rounded-lg px-3 py-2 text-foreground/80 truncate">
          {value}
        </code>
        <button
          onClick={copy}
          className="shrink-0 p-2 rounded-lg border hover:bg-muted/50 transition-colors text-muted-foreground hover:text-foreground"
          title="Copy"
        >
          {copied ? <Check className="w-3.5 h-3.5 text-emerald-400" /> : <Copy className="w-3.5 h-3.5" />}
        </button>
      </div>
    </div>
  )
}

export default function ProjectSettingsPage() {
  const { projectId: paramId } = useParams() as any
  const { projectId: storeId, projectName, tenantId, setProject, clearProject } = useStore()
  const projectId = paramId ?? storeId
  const router = useRouter()
  const queryClient = useQueryClient()

  const [newName, setNewName] = useState(projectName ?? '')
  const [deleteConfirm, setDeleteConfirm] = useState('')

  const renameMutation = useMutation({
    mutationFn: (name: string) =>
      apiFetch(`/projects/${projectId}`, { method: 'PATCH', body: JSON.stringify({ name }) }),
    onSuccess: () => {
      setProject(projectId, newName)
      queryClient.invalidateQueries({ queryKey: ['projects'] })
    },
  })

  const deleteMutation = useMutation({
    mutationFn: () => apiFetch(`/projects/${projectId}`, { method: 'DELETE' }),
    onSuccess: () => {
      clearProject()
      queryClient.invalidateQueries({ queryKey: ['projects'] })
      router.push('/dashboard')
    },
  })

  const API_BASE = process.env.NEXT_PUBLIC_API_URL ?? 'http://localhost:4000'

  return (
    <div className="flex flex-col h-full">
      <PageHeader
        title="Project Settings"
        description={projectName ?? projectId}
        breadcrumbs={[
          { label: 'Projects', href: '/dashboard' },
          { label: projectName ?? projectId ?? '…', href: `/dashboard/projects/${projectId}/overview` },
          { label: 'Settings' },
        ]}
      />

      <div className="flex-1 overflow-y-auto">
        <div className="p-6 max-w-2xl mx-auto space-y-8">

          {/* Project info */}
          <section>
            <h2 className="text-sm font-semibold mb-4">Project Information</h2>
            <div className="rounded-xl border bg-card p-5 space-y-4">
              <CopyField label="Project ID" value={projectId ?? '—'} />
              {tenantId && <CopyField label="Tenant ID" value={tenantId} />}
              <CopyField label="API base URL" value={`${API_BASE}/flux/api`} />
            </div>
          </section>

          <Separator />

          {/* Rename */}
          <section>
            <h2 className="text-sm font-semibold mb-1">Rename Project</h2>
            <p className="text-xs text-muted-foreground mb-4">Update the display name for this project.</p>
            <div className="rounded-xl border bg-card p-5 space-y-4">
              <div className="space-y-1.5">
                <Label htmlFor="proj-name">Project name</Label>
                <Input
                  id="proj-name"
                  value={newName}
                  onChange={(e) => setNewName(e.target.value)}
                  placeholder="my-project"
                />
              </div>
              {renameMutation.isError && (
                <p className="text-sm text-destructive">{renameMutation.error.message}</p>
              )}
              {renameMutation.isSuccess && (
                <p className="text-sm text-emerald-500">Project renamed successfully.</p>
              )}
              <Button
                size="sm"
                onClick={() => renameMutation.mutate(newName.trim())}
                disabled={!newName.trim() || newName === projectName || renameMutation.isPending}
              >
                {renameMutation.isPending ? (
                  <><RefreshCw className="w-3.5 h-3.5 animate-spin" /> Saving…</>
                ) : (
                  'Save changes'
                )}
              </Button>
            </div>
          </section>

          <Separator />

          {/* Danger zone */}
          <section>
            <h2 className="text-sm font-semibold text-destructive mb-1">Danger Zone</h2>
            <p className="text-xs text-muted-foreground mb-4">
              Permanent actions that cannot be undone.
            </p>
            <div className="rounded-xl border border-destructive/30 bg-destructive/[0.03] p-5 space-y-4">
              <div className="flex items-start justify-between gap-4">
                <div>
                  <p className="text-sm font-medium">Delete this project</p>
                  <p className="text-xs text-muted-foreground mt-0.5">
                    Permanently deletes all functions, secrets, routes, and data associated with this project.
                  </p>
                </div>
                <Trash2 className="w-4 h-4 text-destructive/60 shrink-0 mt-0.5" />
              </div>
              <div className="space-y-1.5">
                <Label htmlFor="delete-confirm" className="text-xs">
                  Type <span className="font-mono font-bold text-foreground">{projectName ?? projectId}</span> to confirm
                </Label>
                <Input
                  id="delete-confirm"
                  placeholder={projectName ?? projectId ?? ''}
                  value={deleteConfirm}
                  onChange={(e) => setDeleteConfirm(e.target.value)}
                  className="border-destructive/30 focus-visible:ring-destructive/30"
                />
              </div>
              <Button
                variant="destructive"
                size="sm"
                disabled={deleteConfirm !== (projectName ?? projectId) || deleteMutation.isPending}
                onClick={() => deleteMutation.mutate()}
              >
                {deleteMutation.isPending ? 'Deleting…' : 'Delete project'}
              </Button>
            </div>
          </section>
        </div>
      </div>
    </div>
  )
}

