'use client'

import { useEffect, useState, useCallback } from 'react'

import { useRouter } from 'next/navigation'
import { Command } from 'cmdk'
import { Dialog } from '@radix-ui/react-dialog'
import {
  Code2, Globe, Bell, GitBranch, Clock, Database,
  Terminal, Share2, ShieldCheck, KeyRound, ScrollText, Activity,
  Network, Settings, ListChecks, BarChart2, Puzzle, LayoutDashboard,
  Search,
} from 'lucide-react'
import { cn } from '@/lib/utils'

// ─── Types ────────────────────────────────────────────────────────────────────

interface CmdItem {
  id: string
  label: string
  group: string
  icon: React.ComponentType<{ className?: string }>
  shortcut?: string
  action: () => void
}

// ─── CommandPalette ───────────────────────────────────────────────────────────

export function CommandPalette() {
  const [open, setOpen] = useState(false)
  const router = useRouter()

  // Open on Cmd+K / Ctrl+K
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === 'k') {
        e.preventDefault()
        setOpen(o => !o)
      }
    }
    window.addEventListener('keydown', handler)
    return () => window.removeEventListener('keydown', handler)
  }, [])

  const items: CmdItem[] = [
    // Overview
    { id: 'overview',     label: 'Overview',       group: 'Other',         icon: LayoutDashboard, action: () => router.push('/dashboard') },
    // Runtime
    { id: 'functions',    label: 'Functions',      group: 'Runtime',       icon: Code2,        action: () => router.push('/dashboard/functions') },
    { id: 'routes',       label: 'Routes',         group: 'Runtime',       icon: Globe,        action: () => router.push('/dashboard/routes') },
    { id: 'events',       label: 'Events',         group: 'Runtime',       icon: Bell,         action: () => router.push('/dashboard/events') },
    { id: 'workflows',    label: 'Workflows',      group: 'Runtime',       icon: GitBranch,    action: () => router.push('/dashboard/workflows') },
    { id: 'cron',         label: 'Cron Jobs',      group: 'Runtime',       icon: Clock,        action: () => router.push('/dashboard/cron') },
    { id: 'queues',       label: 'Queues',         group: 'Runtime',       icon: ListChecks,   action: () => router.push('/dashboard/queue') },
    // Data
    { id: 'data',         label: 'Tables',         group: 'Data',          icon: Database,     action: () => router.push('/dashboard/data') },
    { id: 'query',        label: 'Query Explorer', group: 'Data',          icon: Terminal,     action: () => router.push('/dashboard/query') },
    { id: 'schema',       label: 'Schema Graph',   group: 'Data',          icon: Share2,       action: () => router.push('/dashboard/schema') },
    // Security
    { id: 'secrets',      label: 'Secrets',        group: 'Security',      icon: ShieldCheck,  action: () => router.push('/dashboard/secrets') },
    { id: 'api-keys',     label: 'API Keys',       group: 'Security',      icon: KeyRound,     action: () => router.push('/dashboard/api-keys') },
    // Observability
    { id: 'logs',         label: 'Logs',           group: 'Observability', icon: ScrollText,   action: () => router.push('/dashboard/logs') },
    { id: 'traces',       label: 'Traces',         group: 'Observability', icon: Activity,     action: () => router.push('/dashboard/traces') },
    { id: 'monitor',      label: 'Monitor',        group: 'Observability', icon: BarChart2,    action: () => router.push('/dashboard/monitor') },
    { id: 'topology',     label: 'Topology',       group: 'Observability', icon: Network,      action: () => router.push('/dashboard/topology') },
    // Other
    { id: 'integrations', label: 'Integrations',   group: 'Other',         icon: Puzzle,       action: () => router.push('/dashboard/integrations') },
    { id: 'settings',     label: 'Settings',       group: 'Other',         icon: Settings,     action: () => router.push('/dashboard/settings') },
  ]

  // Group items
  const groups = Array.from(new Set(items.map(i => i.group)))

  const handleSelect = useCallback((item: CmdItem) => {
    item.action()
    setOpen(false)
  }, [])

  return (
    <>
      {/* Trigger hint shown in sidebar (optional button) */}
      <button
        onClick={() => setOpen(true)}
        className="hidden"
        aria-label="Open command palette"
      />

      <Dialog open={open} onOpenChange={setOpen}>
        {open && (
          <div className="fixed inset-0 z-[200] flex items-start justify-center pt-[20vh]">
            {/* Backdrop */}
            <div
              className="absolute inset-0 bg-black/60 backdrop-blur-sm"
              onClick={() => setOpen(false)}
            />

            {/* Panel */}
            <div className="relative z-10 w-full max-w-lg mx-4">
              <Command
                className="rounded-xl border border-border bg-zinc-950 shadow-2xl overflow-hidden"
                shouldFilter={true}
              >
                <div className="flex items-center gap-2 border-b border-border px-4 py-3">
                  <Search className="w-4 h-4 text-muted-foreground shrink-0" />
                  <Command.Input
                    className="flex-1 bg-transparent text-sm outline-none placeholder:text-muted-foreground"
                    placeholder="Jump to…"
                    autoFocus
                  />
                  <kbd className="text-xs text-muted-foreground/60 border border-border rounded px-1.5 py-0.5">
                    esc
                  </kbd>
                </div>

                <Command.List className="max-h-80 overflow-y-auto p-2 space-y-1">
                  <Command.Empty className="py-8 text-center text-sm text-muted-foreground">
                    No results found.
                  </Command.Empty>

                  {groups.map(group => (
                    <Command.Group
                      key={group}
                      heading={group}
                      className="[&_[cmdk-group-heading]]:text-xs [&_[cmdk-group-heading]]:text-muted-foreground/60 [&_[cmdk-group-heading]]:px-2 [&_[cmdk-group-heading]]:py-1.5 [&_[cmdk-group-heading]]:font-medium [&_[cmdk-group-heading]]:uppercase [&_[cmdk-group-heading]]:tracking-wider"
                    >
                      {items
                        .filter(i => i.group === group)
                        .map(item => {
                          const Icon = item.icon
                          return (
                            <Command.Item
                              key={item.id}
                              value={item.label}
                              onSelect={() => handleSelect(item)}
                              className={cn(
                                'flex items-center gap-2.5 px-2 py-2 rounded-lg text-sm cursor-pointer',
                                'aria-selected:bg-white/10 aria-selected:text-foreground',
                                'text-muted-foreground transition-colors',
                              )}
                            >
                              <Icon className="w-4 h-4 shrink-0" />
                              {item.label}
                              {item.shortcut && (
                                <kbd className="ml-auto text-xs text-muted-foreground/50 border border-border rounded px-1.5">
                                  {item.shortcut}
                                </kbd>
                              )}
                            </Command.Item>
                          )
                        })}
                    </Command.Group>
                  ))}
                </Command.List>
              </Command>
            </div>
          </div>
        )}
      </Dialog>
    </>
  )
}
