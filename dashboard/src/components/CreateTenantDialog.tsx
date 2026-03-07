import { useState } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import {
  Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription, DialogFooter,
} from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Button } from '@/components/ui/button'
import { Label } from '@/components/ui/label'
import { apiFetch } from '@/lib/api'
import { useTenant } from '@/hooks/useTenant'

interface Props {
  open: boolean
  onOpenChange: (open: boolean) => void
}

export function CreateTenantDialog({ open, onOpenChange }: Props) {
  const [name, setName] = useState('')
  const queryClient = useQueryClient()
  const { switchTenant } = useTenant()

  const mutation = useMutation({
    mutationFn: (name: string) =>
      apiFetch<{ tenant_id: string }>('/tenants', {
        method: 'POST',
        body: JSON.stringify({ name }),
      }),
    onSuccess: (data) => {
      switchTenant(data.tenant_id, name)
      queryClient.invalidateQueries({ queryKey: ['tenants'] })
      setName('')
      onOpenChange(false)
    },
  })

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Create tenant</DialogTitle>
          <DialogDescription>
            A tenant is your organization or workspace. Projects live inside tenants.
          </DialogDescription>
        </DialogHeader>
        <div className="space-y-3">
          <Label htmlFor="tenant-name">Tenant name</Label>
          <Input
            id="tenant-name"
            placeholder="Acme Inc"
            value={name}
            onChange={(e) => setName(e.target.value)}
            onKeyDown={(e) => e.key === 'Enter' && name.trim() && mutation.mutate(name.trim())}
          />
          {mutation.isError && (
            <p className="text-sm text-destructive">{mutation.error.message}</p>
          )}
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>Cancel</Button>
          <Button
            onClick={() => mutation.mutate(name.trim())}
            disabled={!name.trim() || mutation.isPending}
          >
            {mutation.isPending ? 'Creating…' : 'Create tenant'}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
