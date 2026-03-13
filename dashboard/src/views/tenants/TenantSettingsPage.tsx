'use client'

import { useState } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { UserPlus, Trash2, Users } from 'lucide-react'
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
import { Avatar, AvatarFallback } from '@/components/ui/avatar'
import { Separator } from '@/components/ui/separator'

interface Member { user_id: string; email: string; role: string }

const ROLE_VARIANT: Record<string, 'default' | 'secondary' | 'outline'> = {
  owner: 'default',
  admin: 'secondary',
  member: 'outline',
}

export default function TenantSettingsPage() {
  const { tenantId, tenantName } = useStore()
  const queryClient = useQueryClient()
  const [inviteOpen, setInviteOpen] = useState(false)
  const [email, setEmail] = useState('')
  const [role, setRole] = useState('member')

  const { data, isLoading } = useQuery({
    queryKey: ['members', tenantId],
    queryFn: () => apiFetch<{ members: Member[] }>(`/tenants/${tenantId}/members`),
    enabled: !!tenantId,
  })

  const inviteMutation = useMutation({
    mutationFn: () =>
      apiFetch(`/tenants/${tenantId}/members`, {
        method: 'POST',
        body: JSON.stringify({ email, role }),
      }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['members'] })
      setEmail('')
      setRole('member')
      setInviteOpen(false)
    },
  })

  const removeMutation = useMutation({
    mutationFn: (userId: string) =>
      apiFetch(`/tenants/${tenantId}/members/${userId}`, { method: 'DELETE' }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['members'] }),
  })

  const members = data?.members ?? []

  return (
    <div className="p-8 max-w-3xl mx-auto">
      <h1 className="text-2xl font-bold mb-1">Tenant Settings</h1>
      <p className="text-sm text-muted-foreground mb-8">{tenantName}</p>

      {/* Tenant Info */}
      <section className="mb-8">
        <h2 className="text-base font-semibold mb-4">Tenant Info</h2>
        <div className="rounded-xl border p-4 space-y-3">
          <div>
            <p className="text-xs text-muted-foreground mb-0.5">Name</p>
            <p className="text-sm font-medium">{tenantName ?? '—'}</p>
          </div>
          <div>
            <p className="text-xs text-muted-foreground mb-0.5">ID</p>
            <p className="text-xs font-mono text-muted-foreground">{tenantId ?? '—'}</p>
          </div>
        </div>
      </section>

      <Separator className="mb-8" />

      {/* Members */}
      <section className="mb-8">
        <div className="flex items-center justify-between mb-4">
          <h2 className="text-base font-semibold">Members</h2>
          <Button size="sm" onClick={() => setInviteOpen(true)}>
            <UserPlus className="w-3.5 h-3.5" /> Invite
          </Button>
        </div>

        {isLoading ? (
          <div className="space-y-2">
            {[...Array(2)].map((_, i) => (
              <div key={i} className="h-12 rounded-xl bg-muted/40 animate-pulse" />
            ))}
          </div>
        ) : members.length === 0 ? (
          <div className="rounded-xl border border-dashed p-8 text-center">
            <Users className="w-7 h-7 mx-auto mb-3 text-muted-foreground/40" />
            <p className="text-sm text-muted-foreground">No members found.</p>
          </div>
        ) : (
          <div className="rounded-xl border overflow-hidden">
            {members.map((m) => (
              <div key={m.user_id} className="flex items-center gap-3 px-4 py-3 border-b last:border-0 hover:bg-muted/20 group transition-colors">
                <Avatar className="w-8 h-8 shrink-0">
                  <AvatarFallback className="text-xs">
                    {m.email.slice(0, 2).toUpperCase()}
                  </AvatarFallback>
                </Avatar>
                <div className="flex-1 min-w-0">
                  <p className="text-sm font-medium truncate">{m.email}</p>
                </div>
                <Badge variant={ROLE_VARIANT[m.role] ?? 'outline'} className="text-xs capitalize">
                  {m.role}
                </Badge>
                {m.role !== 'owner' && (
                  <button
                    className="opacity-0 group-hover:opacity-100 transition-opacity p-1.5 rounded hover:bg-destructive/10 text-muted-foreground hover:text-destructive"
                    onClick={() => {
                      if (confirm(`Remove ${m.email}?`)) removeMutation.mutate(m.user_id)
                    }}
                  >
                    <Trash2 className="w-3.5 h-3.5" />
                  </button>
                )}
              </div>
            ))}
          </div>
        )}
      </section>

      <Separator className="mb-8" />

      {/* Danger zone */}
      <section>
        <h2 className="text-base font-semibold text-destructive mb-4">Danger Zone</h2>
        <div className="rounded-xl border border-destructive/30 p-4 flex items-center justify-between">
          <div>
            <p className="text-sm font-medium">Delete tenant</p>
            <p className="text-xs text-muted-foreground mt-0.5">
              Permanently removes this tenant and all its projects.
            </p>
          </div>
          <Button
            variant="destructive"
            size="sm"
            onClick={() => {
              if (confirm('Delete this tenant? This action CANNOT be undone.')) {
                apiFetch(`/tenants/${tenantId}`, { method: 'DELETE' }).then(() => {
                  useStore.getState().clear()
                  window.location.href = '/dashboard'
                })
              }
            }}
          >
            Delete tenant
          </Button>
        </div>
      </section>

      {/* Invite Dialog */}
      <Dialog open={inviteOpen} onOpenChange={setInviteOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Invite member</DialogTitle>
            <DialogDescription>
              Invite an existing Fluxbase user to this tenant.
            </DialogDescription>
          </DialogHeader>
          <div className="space-y-4">
            <div className="space-y-2">
              <Label>Email address</Label>
              <Input
                type="email"
                placeholder="dev@acme.com"
                value={email}
                onChange={(e) => setEmail(e.target.value)}
              />
            </div>
            <div className="space-y-2">
              <Label>Role</Label>
              <Select value={role} onValueChange={setRole}>
                <SelectTrigger>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="admin">Admin</SelectItem>
                  <SelectItem value="member">Member</SelectItem>
                </SelectContent>
              </Select>
            </div>
            {inviteMutation.isError && (
              <p className="text-sm text-destructive">{inviteMutation.error.message}</p>
            )}
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setInviteOpen(false)}>Cancel</Button>
            <Button onClick={() => inviteMutation.mutate()} disabled={!email.trim() || inviteMutation.isPending}>
              {inviteMutation.isPending ? 'Inviting…' : 'Send invite'}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  )
}
