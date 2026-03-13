'use client'

import { useState } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { CheckCircle2, XCircle, ExternalLink, Unplug } from 'lucide-react'
import { apiFetch } from '@/lib/api'
import { useStore } from '@/state/tenantStore'
import { PageHeader } from '@/components/layout/PageHeader'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import {
  Dialog, DialogContent, DialogHeader, DialogTitle,
  DialogDescription, DialogFooter,
} from '@/components/ui/dialog'

// ── Types ──────────────────────────────────────────────────────────────────

interface Tool {
  name: string
  provider: string
  label: string
  description: string
  connected: boolean
}

// ── Provider grouping ──────────────────────────────────────────────────────
//
// Group tools by provider for the card layout.

interface ProviderGroup {
  provider: string
  displayName: string
  tools: Tool[]
  connected: boolean
}

const PROVIDER_LABELS: Record<string, string> = {
  slack:        'Slack',
  github:       'GitHub',
  gmail:        'Gmail',
  linear:       'Linear',
  notion:       'Notion',
  jira:         'Jira',
  airtable:     'Airtable',
  googlesheets: 'Google Sheets',
  stripe:       'Stripe',
}

// Deterministic color from provider name for the avatar circle
const PROVIDER_COLORS: Record<string, string> = {
  slack:        'bg-[#4A154B] text-white',
  github:       'bg-[#24292e] text-white',
  gmail:        'bg-[#EA4335] text-white',
  linear:       'bg-[#5E6AD2] text-white',
  notion:       'bg-[#000000] text-white',
  jira:         'bg-[#0052CC] text-white',
  airtable:     'bg-[#FCB400] text-black',
  googlesheets: 'bg-[#0F9D58] text-white',
  stripe:       'bg-[#635BFF] text-white',
}

function groupByProvider(tools: Tool[]): ProviderGroup[] {
  const map = new Map<string, ProviderGroup>()
  for (const t of tools) {
    if (!map.has(t.provider)) {
      map.set(t.provider, {
        provider: t.provider,
        displayName: PROVIDER_LABELS[t.provider] ?? t.provider,
        tools: [],
        connected: t.connected,
      })
    }
    map.get(t.provider)!.tools.push(t)
    if (t.connected) map.get(t.provider)!.connected = true
  }
  return Array.from(map.values()).sort((a, b) => {
    // Connected first, then alphabetical
    if (a.connected && !b.connected) return -1
    if (!a.connected && b.connected) return 1
    return a.displayName.localeCompare(b.displayName)
  })
}

// ── Page ───────────────────────────────────────────────────────────────────

export default function IntegrationsPage() {
  const { projectId, projectName } = useStore()
  const queryClient   = useQueryClient()

  const [disconnectTarget, setDisconnectTarget] = useState<string | null>(null)

  // Fetch all tools (with connected flag)
  const { data: toolsData, isLoading } = useQuery({
    queryKey: ['tools', projectId],
    queryFn:  () => apiFetch<{ tools: Tool[] }>('/tools'),
    enabled:  !!projectId,
  })

  // Connect mutation — gets OAuth URL and redirects window
  const connectMutation = useMutation({
    mutationFn: (provider: string) =>
      apiFetch<{ oauth_url: string }>(`/tools/connect/${provider}`, {
        method: 'POST',
        body:   JSON.stringify({}),
      }),
    onSuccess: (data) => {
      // Redirect the user to the OAuth provider page
      window.location.href = data.oauth_url
    },
  })

  // Disconnect mutation
  const disconnectMutation = useMutation({
    mutationFn: (provider: string) =>
      apiFetch(`/tools/disconnect/${provider}`, { method: 'DELETE' }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['tools'] })
      setDisconnectTarget(null)
    },
  })

  const tools    = toolsData?.tools ?? []
  const groups   = groupByProvider(tools)
  const connected = groups.filter(g => g.connected).length

  return (
    <div className="flex flex-col h-full">
      <PageHeader
        title="Integrations"
        description={connected > 0 ? `${connected} connected` : 'Connect external services to use them in your functions with ctx.tools.run()'}
        breadcrumbs={[
          { label: 'Projects', href: '/dashboard' },
          { label: projectName ?? projectId ?? '…', href: `/dashboard/projects/${projectId}/overview` },
          { label: 'Integrations' },
        ]}
      />
      <div className="flex-1 overflow-y-auto">
      <div className="p-8 max-w-5xl mx-auto">

      {/* Grid */}
      {isLoading ? (
        <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4">
          {[...Array(9)].map((_, i) => (
            <div key={i} className="h-44 rounded-xl bg-muted/40 animate-pulse" />
          ))}
        </div>
      ) : (
        <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4">
          {groups.map((group) => (
            <ProviderCard
              key={group.provider}
              group={group}
              onConnect={() => connectMutation.mutate(group.provider)}
              onDisconnect={() => setDisconnectTarget(group.provider)}
              connecting={connectMutation.isPending && connectMutation.variables === group.provider}
            />
          ))}
        </div>
      )}

      {/* Disconnect confirmation dialog */}
      <Dialog
        open={!!disconnectTarget}
        onOpenChange={(open) => !open && setDisconnectTarget(null)}
      >
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Disconnect {PROVIDER_LABELS[disconnectTarget ?? ''] ?? disconnectTarget}?</DialogTitle>
            <DialogDescription>
              This will remove the connection. Functions calling{' '}
              <code className="font-mono text-xs bg-muted px-1 rounded">{disconnectTarget}</code>{' '}
              tools will fail until you reconnect.
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setDisconnectTarget(null)}>
              Cancel
            </Button>
            <Button
              variant="destructive"
              onClick={() => disconnectTarget && disconnectMutation.mutate(disconnectTarget)}
              disabled={disconnectMutation.isPending}
            >
              {disconnectMutation.isPending ? 'Disconnecting…' : 'Disconnect'}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
      </div>
      </div>
    </div>
  )
}

// ── Provider Card ──────────────────────────────────────────────────────────

interface ProviderCardProps {
  group:       ProviderGroup
  onConnect:   () => void
  onDisconnect: () => void
  connecting:  boolean
}

function ProviderCard({ group, onConnect, onDisconnect, connecting }: ProviderCardProps) {
  const colorClass = PROVIDER_COLORS[group.provider] ?? 'bg-primary/20 text-primary'
  const initials = group.displayName.slice(0, 2).toUpperCase()

  return (
    <div className="relative flex flex-col rounded-xl border bg-card p-5 gap-3 hover:border-primary/40 transition-colors">
      {/* Provider header */}
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-3">
          <div className={`w-9 h-9 rounded-lg flex items-center justify-center text-xs font-bold shrink-0 ${colorClass}`}>
            {initials}
          </div>
          <div>
            <div className="font-semibold text-sm">{group.displayName}</div>
            <div className="text-xs text-muted-foreground">{group.tools.length} action{group.tools.length !== 1 ? 's' : ''}</div>
          </div>
        </div>
        {group.connected ? (
          <Badge className="bg-green-500/15 text-green-600 border-green-500/30 gap-1 text-xs">
            <CheckCircle2 className="w-3 h-3" /> Connected
          </Badge>
        ) : (
          <Badge variant="outline" className="text-xs text-muted-foreground gap-1">
            <XCircle className="w-3 h-3" /> Not connected
          </Badge>
        )}
      </div>

      {/* Tool list (first 3) */}
      <ul className="space-y-1 flex-1">
        {group.tools.slice(0, 3).map((t) => (
          <li key={t.name} className="text-xs text-muted-foreground truncate">
            <span className="font-mono text-foreground/70">{t.name}</span>
            {' — '}{t.description}
          </li>
        ))}
        {group.tools.length > 3 && (
          <li className="text-xs text-muted-foreground">
            +{group.tools.length - 3} more actions
          </li>
        )}
      </ul>

      {/* Action button */}
      <div className="flex gap-2 pt-1">
        {group.connected ? (
          <Button
            size="sm"
            variant="outline"
            className="flex-1 text-destructive border-destructive/30 hover:bg-destructive/10"
            onClick={onDisconnect}
          >
            <Unplug className="w-3.5 h-3.5" /> Disconnect
          </Button>
        ) : (
          <Button
            size="sm"
            className="flex-1"
            onClick={onConnect}
            disabled={connecting}
          >
            {connecting ? (
              'Connecting…'
            ) : (
              <>
                <ExternalLink className="w-3.5 h-3.5" /> Connect
              </>
            )}
          </Button>
        )}
      </div>
    </div>
  )
}
