'use client'

import { useEffect, useState, useCallback } from 'react'
import Link from 'next/link'
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

// basePath must match next.config.ts `basePath`
const BASE_PATH = '/flux'

interface CmdItem {
  id: string
  label: string
  group: string
  icon: React.ComponentType<{ className?: string }>
  shortcut?: string
  href: string
}

// ─── CommandPalette ───────────────────────────────────────────────────────────

export function CommandPalette() {
  const [open, setOpen] = useState(false)

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
    { id: 'overview',     label: 'Overview',       group: 'Other',         icon: LayoutDashboard, href: '/dashboard' },
    // Runtime
    { id: 'functions',    label: 'Functions',      group: 'Runtime',       icon: Code2,        href: '/dashboard/functions' },
    { id: 'routes',       label: 'Routes',         group: 'Runtime',       icon: Globe,        href: '/dashboard/routes' },
    { id: 'events',       label: 'Events',         group: 'Runtime',       icon: Bell,         href: '/dashboard/events' },
    { id: 'workflows',    label: 'Workflows',      group: 'Runtime',       icon: GitBranch,    href: '/dashboard/workflows' },
    { id: 'cron',         label: 'Cron Jobs',      group: 'Runtime',       icon: Clock,        href: '/dashboard/cron' },
    { id: 'queues',       label: 'Queues',         group: 'Runtime',       icon: ListChecks,   href: '/dashboard/queue' },
    // Data
    { id: 'data',         label: 'Tables',         group: 'Data',          icon: Database,     href: '/dashboard/data' },
    { id: 'query',        label: 'Query Explorer', group: 'Data',          icon: Terminal,     href: '/dashboard/query' },
    { id: 'schema',       label: 'Schema Graph',   group: 'Data',          icon: Share2,       href: '/dashboard/schema' },
    // Security
    { id: 'secrets',      label: 'Secrets',        group: 'Security',      icon: ShieldCheck,  href: '/dashboard/secrets' },
    { id: 'api-keys',     label: 'API Keys',       group: 'Security',      icon: KeyRound,     href: '/dashboard/api-keys' },
    // Observability
    { id: 'logs',         label: 'Logs',           group: 'Observability', icon: ScrollText,   href: '/dashboard/logs' },
    { id: 'traces',       label: 'Traces',         group: 'Observability', icon: Activity,     href: '/dashboard/traces' },
    { id: 'monitor',      label: 'Monitor',        group: 'Observability', icon: BarChart2,    href: '/dashboard/monitor' },
    { id: 'topology',     label: 'Topology',       group: 'Observability', icon: Network,      href: '/dashboard/topology' },
    // Other
    { id: 'integrations', label: 'Integrations',   group: 'Other',         icon: Puzzle,       href: '/dashboard/integrations' },
    { id: 'settings',     label: 'Settings',       group: 'Other',         icon: Settings,     href: '/dashboard/settings' },
  ]

  // Group items
  const groups = Array.from(new Set(items.map(i => i.group)))

  const handleSelect = useCallback((item: CmdItem) => {
    setOpen(false)
    window.location.href = BASE_PATH + item.href
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
                              <Link href={item.href} className="contents" tabIndex={-1} onClick={() => setOpen(false)}>
                                <Icon className="w-4 h-4 shrink-0" />
                                {item.label}
                                {item.shortcut && (
                                  <kbd className="ml-auto text-xs text-muted-foreground/50 border border-border rounded px-1.5">
                                    {item.shortcut}
                                  </kbd>
                                )}
                              </Link>
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
