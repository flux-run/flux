import { useState } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { Plus, ShieldCheck, Trash2, Eye, EyeOff } from 'lucide-react'
import { apiFetch } from '@/lib/api'
import { useStore } from '@/state/tenantStore'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import {
  Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription, DialogFooter,
} from '@/components/ui/dialog'
import { Badge } from '@/components/ui/badge'

interface Secret { id: string; key: string; scope: string; created_at: string }

export default function SecretsPage() {
  const { projectId } = useStore()
  const queryClient = useQueryClient()
  const [createOpen, setCreateOpen] = useState(false)
  const [key, setKey] = useState('')
  const [value, setValue] = useState('')
  const [showValue, setShowValue] = useState(false)

  const { data, isLoading } = useQuery({
    queryKey: ['secrets', projectId],
    queryFn: () => apiFetch<{ secrets: Secret[] }>('/secrets'),
    enabled: !!projectId,
  })

  const createMutation = useMutation({
    mutationFn: () =>
      apiFetch('/secrets', {
        method: 'POST',
        body: JSON.stringify({ key, value }),
      }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['secrets'] })
      setKey('')
      setValue('')
      setCreateOpen(false)
    },
  })

  const deleteMutation = useMutation({
    mutationFn: (k: string) => apiFetch(`/secrets/${k}`, { method: 'DELETE' }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['secrets'] }),
  })

  const secrets = data?.secrets ?? []

  return (
    <div className="p-8 max-w-4xl mx-auto">
      <div className="flex items-center justify-between mb-8">
        <div>
          <h1 className="text-2xl font-bold">Secrets</h1>
          <p className="text-sm text-muted-foreground mt-0.5">
            Environment variables injected at runtime. Values are never shown after creation.
          </p>
        </div>
        <Button onClick={() => setCreateOpen(true)}>
          <Plus className="w-4 h-4" /> Add secret
        </Button>
      </div>

      {isLoading ? (
        <div className="space-y-2">
          {[...Array(3)].map((_, i) => (
            <div key={i} className="h-12 rounded-xl bg-muted/40 animate-pulse" />
          ))}
        </div>
      ) : secrets.length === 0 ? (
        <div className="rounded-xl border border-dashed p-10 text-center">
          <ShieldCheck className="w-8 h-8 mx-auto mb-3 text-muted-foreground/40" />
          <p className="font-medium mb-1">No secrets yet</p>
          <p className="text-sm text-muted-foreground mb-4">
            Store API keys, tokens, and database URLs securely.
          </p>
          <Button onClick={() => setCreateOpen(true)}>
            <Plus className="w-4 h-4" /> Add secret
          </Button>
        </div>
      ) : (
        <div className="rounded-xl border overflow-hidden">
          {/* Header */}
          <div className="grid grid-cols-[1fr_100px_1fr_48px] gap-4 px-4 py-2 bg-muted/30 border-b">
            <p className="text-xs font-semibold text-muted-foreground uppercase tracking-wide">Key</p>
            <p className="text-xs font-semibold text-muted-foreground uppercase tracking-wide">Scope</p>
            <p className="text-xs font-semibold text-muted-foreground uppercase tracking-wide">Created</p>
            <span />
          </div>
          {secrets.map((s) => (
            <div key={s.id} className="grid grid-cols-[1fr_100px_1fr_48px] gap-4 items-center px-4 py-3 border-b last:border-0 hover:bg-muted/20 group transition-colors">
              <p className="font-mono text-sm font-medium">{s.key}</p>
              <Badge variant="outline" className="w-fit font-mono text-xs">{s.scope}</Badge>
              <p className="text-xs text-muted-foreground">
                {s.created_at ? new Date(s.created_at).toLocaleDateString() : '—'}
              </p>
              <button
                className="opacity-0 group-hover:opacity-100 transition-opacity p-1.5 rounded hover:bg-destructive/10 text-muted-foreground hover:text-destructive"
                onClick={() => {
                  if (confirm(`Delete secret "${s.key}"?`)) deleteMutation.mutate(s.key)
                }}
              >
                <Trash2 className="w-3.5 h-3.5" />
              </button>
            </div>
          ))}
        </div>
      )}

      <Dialog open={createOpen} onOpenChange={setCreateOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Add secret</DialogTitle>
            <DialogDescription>
              The value will be encrypted at rest. It cannot be retrieved after saving.
            </DialogDescription>
          </DialogHeader>
          <div className="space-y-4">
            <div className="space-y-2">
              <Label>Key</Label>
              <Input
                className="font-mono"
                placeholder="STRIPE_SECRET_KEY"
                value={key}
                onChange={(e) => setKey(e.target.value.toUpperCase().replace(/\s/g, '_'))}
              />
            </div>
            <div className="space-y-2">
              <Label>Value</Label>
              <div className="relative">
                <Input
                  type={showValue ? 'text' : 'password'}
                  className="font-mono pr-9"
                  placeholder="sk_live_…"
                  value={value}
                  onChange={(e) => setValue(e.target.value)}
                />
                <button
                  type="button"
                  className="absolute right-2.5 top-1/2 -translate-y-1/2 text-muted-foreground hover:text-foreground"
                  onClick={() => setShowValue((v) => !v)}
                >
                  {showValue ? <EyeOff className="w-4 h-4" /> : <Eye className="w-4 h-4" />}
                </button>
              </div>
              <p className="text-xs text-muted-foreground">
                ⚠ This value will never be shown again after saving.
              </p>
            </div>
            {createMutation.isError && (
              <p className="text-sm text-destructive">{createMutation.error.message}</p>
            )}
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setCreateOpen(false)}>Cancel</Button>
            <Button
              onClick={() => createMutation.mutate()}
              disabled={!key.trim() || !value.trim() || createMutation.isPending}
            >
              {createMutation.isPending ? 'Saving…' : 'Save secret'}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  )
}
