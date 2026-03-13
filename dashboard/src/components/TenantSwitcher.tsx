'use client'

import { useQuery } from '@tanstack/react-query'
import { ChevronsUpDown, Plus, Check, Building2 } from 'lucide-react'
import { apiFetch } from '@/lib/api'
import { useTenant } from '@/hooks/useTenant'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { useState } from 'react'
import { CreateTenantDialog } from './CreateTenantDialog'

interface Tenant {
  id: string
  name: string
  role: string
}

export function TenantSwitcher() {
  const { tenantId, tenantName, switchTenant } = useTenant()
  const [createOpen, setCreateOpen] = useState(false)

  const { data } = useQuery({
    queryKey: ['tenants'],
    queryFn: () => apiFetch<{ tenants: Tenant[] }>('/tenants'),
    staleTime: 30_000,
  })

  const tenants = data?.tenants ?? []

  return (
    <>
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <button className="flex items-center gap-2 w-full px-2 py-2 rounded-lg hover:bg-white/5 transition-colors group text-left">
            <div className="flex items-center justify-center w-7 h-7 rounded-md bg-primary/20 shrink-0">
              <Building2 className="w-3.5 h-3.5 text-primary" />
            </div>
            <div className="flex-1 min-w-0">
              <p className="text-sm font-medium text-sidebar-foreground truncate">
                {tenantName ?? 'Select tenant'}
              </p>
            </div>
            <ChevronsUpDown className="w-3.5 h-3.5 text-sidebar-foreground/50 shrink-0" />
          </button>
        </DropdownMenuTrigger>
        <DropdownMenuContent className="w-56 ml-1" align="start" side="bottom">
          <DropdownMenuLabel className="text-xs text-muted-foreground font-normal">
            Your tenants
          </DropdownMenuLabel>
          <DropdownMenuSeparator />
          {tenants.map((t) => (
            <DropdownMenuItem
              key={t.id}
              onClick={() => switchTenant(t.id, t.name)}
              className="gap-2"
            >
              <div className="flex items-center justify-center w-5 h-5 rounded-sm bg-primary/10 shrink-0">
                <Building2 className="w-3 h-3 text-primary" />
              </div>
              <span className="flex-1 truncate">{t.name}</span>
              {t.id === tenantId && <Check className="w-3.5 h-3.5 text-primary" />}
            </DropdownMenuItem>
          ))}
          <DropdownMenuSeparator />
          <DropdownMenuItem onClick={() => setCreateOpen(true)} className="gap-2 text-muted-foreground">
            <Plus className="w-4 h-4" />
            Create tenant
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>

      <CreateTenantDialog open={createOpen} onOpenChange={setCreateOpen} />
    </>
  )
}
