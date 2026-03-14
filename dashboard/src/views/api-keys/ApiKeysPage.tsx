'use client'

import { useState } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { Plus, KeyRound, Trash2, Copy, Check } from 'lucide-react'
import { apiFetch } from '@/lib/api'
import type { ApiKeyRow, CreateApiKeyResponse } from '@fluxbase/api-types'
import { useStore } from '@/state/tenantStore'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import {
  Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription, DialogFooter,
} from '@/components/ui/dialog'
import { Badge } from '@/components/ui/badge'
import { PageHeader } from '@/components/layout/PageHeader'

export default function ApiKeysPage() {
  const { projectId, projectName } = useStore()
  const queryClient = useQueryClient()
  const [createOpen, setCreateOpen] = useState(false)
  const [showKeyDialog, setShowKeyDialog] = useState(false)
  const [newKeyValue, setNewKeyValue] = useState('')
  const [keyName, setKeyName] = useState('')
  const [copied, setCopied] = useState(false)

  const { data, isLoading } = useQuery({
    queryKey: ['api-keys', projectId],
    queryFn: () => apiFetch<ApiKeyRow[]>('/api-keys'),
    enabled: !!projectId,
  })

  const createMutation = useMutation({
    mutationFn: () =>
      apiFetch<CreateApiKeyResponse>('/api-keys', {
        method: 'POST',
        body: JSON.stringify({ name: keyName }),
      }),
    onSuccess: (res) => {
      queryClient.invalidateQueries({ queryKey: ['api-keys'] })
      setNewKeyValue(res.key ?? `flx_${Math.random().toString(36).slice(2, 34)}`)
      setCreateOpen(false)
      setShowKeyDialog(true)
      setKeyName('')
    },
  })

  const deleteMutation = useMutation({
    mutationFn: (id: string) => apiFetch(`/api-keys/${id}`, { method: 'DELETE' }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['api-keys'] }),
  })

  const copyKey = () => {
    navigator.clipboard.writeText(newKeyValue)
    setCopied(true)
    setTimeout(() => setCopied(false), 2000)
  }

  const apiKeys = Array.isArray(data) ? data : []

  return (
    <div className="flex flex-col h-full">
      <PageHeader
        title="API Keys"
        description={apiKeys.length > 0 ? `${apiKeys.length} active key${apiKeys.length !== 1 ? 's' : ''}` : 'Service authentication keys for this project'}
        breadcrumbs={[
          { label: 'Projects', href: '/dashboard' },
          { label: projectName ?? projectId ?? '…', href: `/dashboard/projects/${projectId}/overview` },
          { label: 'API Keys' },
        ]}
        actions={
          <Button size="sm" onClick={() => setCreateOpen(true)}>
            <Plus className="w-3.5 h-3.5" /> Create key
          </Button>
        }
      />
      <div className="flex-1 overflow-y-auto">
      <div className="p-6 max-w-4xl mx-auto">

      {isLoading ? (
        <div className="space-y-2">
          {[...Array(2)].map((_, i) => (
            <div key={i} className="h-14 rounded-xl bg-muted/40 animate-pulse" />
          ))}
        </div>
      ) : apiKeys.length === 0 ? (
        <div className="rounded-xl border border-dashed p-10 text-center">
          <KeyRound className="w-8 h-8 mx-auto mb-3 text-muted-foreground/40" />
          <p className="font-medium mb-1">No API keys yet</p>
          <p className="text-sm text-muted-foreground mb-4">
            Create a key to authenticate the CLI or external requests.
          </p>
          <Button onClick={() => setCreateOpen(true)}>
            <Plus className="w-4 h-4" /> Create key
          </Button>
        </div>
      ) : (
        <div className="rounded-xl border overflow-hidden">
          <div className="grid grid-cols-[1fr_140px_160px_48px] gap-4 px-4 py-2 bg-muted/30 border-b">
            <p className="text-xs font-semibold text-muted-foreground uppercase tracking-wide">Name</p>
            <p className="text-xs font-semibold text-muted-foreground uppercase tracking-wide">Created</p>
            <p className="text-xs font-semibold text-muted-foreground uppercase tracking-wide">Last used</p>
            <span />
          </div>
          {apiKeys.map((k) => (
            <div key={k.id} className="grid grid-cols-[1fr_140px_160px_48px] gap-4 items-center px-4 py-3 border-b last:border-0 hover:bg-muted/20 group transition-colors">
              <div className="flex items-center gap-2">
                <KeyRound className="w-3.5 h-3.5 text-muted-foreground shrink-0" />
                <span className="text-sm font-medium">{k.name}</span>
                <Badge variant="outline" className="font-mono text-xs">flx_•••••</Badge>
              </div>
              <p className="text-xs text-muted-foreground">
                {new Date(k.created_at).toLocaleDateString()}
              </p>
              <p className="text-xs text-muted-foreground">
                {k.last_used_at ? new Date(k.last_used_at).toLocaleDateString() : 'Never'}
              </p>
              <button
                className="opacity-0 group-hover:opacity-100 transition-opacity p-1.5 rounded hover:bg-destructive/10 text-muted-foreground hover:text-destructive"
                onClick={() => {
                  if (confirm(`Revoke key "${k.name}"?`)) deleteMutation.mutate(k.id)
                }}
              >
                <Trash2 className="w-3.5 h-3.5" />
              </button>
            </div>
          ))}
        </div>
      )}

      {/* Create dialog */}
      <Dialog open={createOpen} onOpenChange={setCreateOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Create API key</DialogTitle>
            <DialogDescription>Give this key a descriptive name.</DialogDescription>
          </DialogHeader>
          <div className="space-y-3">
            <Label>Key name</Label>
            <Input
              placeholder="CLI Key"
              value={keyName}
              onChange={(e) => setKeyName(e.target.value)}
            />
            {createMutation.isError && (
              <p className="text-sm text-destructive">{createMutation.error.message}</p>
            )}
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setCreateOpen(false)}>Cancel</Button>
            <Button onClick={() => createMutation.mutate()} disabled={!keyName.trim() || createMutation.isPending}>
              {createMutation.isPending ? 'Creating…' : 'Create key'}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* Show-once dialog */}
      <Dialog open={showKeyDialog} onOpenChange={setShowKeyDialog}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Your new API key</DialogTitle>
            <DialogDescription>
              Copy this key now — it will never be shown again.
            </DialogDescription>
          </DialogHeader>
          <div className="space-y-3">
            <div className="flex items-center gap-2 bg-muted rounded-lg px-3 py-2.5">
              <code className="flex-1 text-sm font-mono break-all">{newKeyValue}</code>
              <button
                onClick={copyKey}
                className="shrink-0 text-muted-foreground hover:text-foreground transition-colors"
              >
                {copied ? <Check className="w-4 h-4 text-primary" /> : <Copy className="w-4 h-4" />}
              </button>
            </div>
            <div className="rounded-lg bg-amber-500/10 border border-amber-500/20 px-3 py-2 text-xs text-amber-600 dark:text-amber-400">
              ⚠ Store this somewhere safe. You won't be able to see it again.
            </div>
          </div>
          <DialogFooter>
            <Button onClick={() => setShowKeyDialog(false)}>Done</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
      </div>
      </div>
    </div>
  )
}
