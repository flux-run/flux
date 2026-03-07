import { useState } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { Plus, FolderOpen, Trash2, ArrowRight } from 'lucide-react'
import { useNavigate } from 'react-router-dom'
import { apiFetch } from '@/lib/api'
import { useStore } from '@/state/tenantStore'
import { useProject } from '@/hooks/useProject'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
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
  const navigate = useNavigate()
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
      navigate(`/dashboard/projects/${res.project_id}/overview`)
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
    navigate(`/dashboard/projects/${p.id}/overview`)
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
        <div className="rounded-xl border border-dashed p-10 text-center">
          <FolderOpen className="w-8 h-8 mx-auto mb-3 text-muted-foreground/40" />
          <p className="font-medium mb-1">No projects yet</p>
          <p className="text-sm text-muted-foreground mb-4">
            Create a project to start deploying functions.
          </p>
          <Button onClick={() => setCreateOpen(true)}>
            <Plus className="w-4 h-4" />
            Create project
          </Button>
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
