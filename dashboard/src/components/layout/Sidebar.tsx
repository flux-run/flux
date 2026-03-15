'use client'

import { useState } from 'react'
import Link from 'next/link'
import { usePathname } from 'next/navigation'
import { cn } from '@/lib/utils'
import {
  Settings, LayoutDashboard,
  Code2, KeyRound, ShieldCheck, ScrollText, Globe,
  Database, Bell, GitBranch, Clock, Terminal, Share2, Puzzle,
  Activity, Network, Brain, ChevronDown, ListChecks, BarChart2, Search,
} from 'lucide-react'
import { FluxLogo } from '@/components/FluxLogo'
import { useAuth } from '@/hooks/useAuth'
import { Avatar, AvatarFallback } from '@/components/ui/avatar'
import {
  DropdownMenu, DropdownMenuContent, DropdownMenuItem,
  DropdownMenuSeparator, DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { CommandPalette } from '@/components/CommandPalette'

// ─── NavItem ──────────────────────────────────────────────────────────────────

function NavItem({
  href,
  Icon,
  label,
  indent = false,
}: {
  href: string
  Icon: React.ComponentType<{ className?: string }>
  label: string
  indent?: boolean
}) {
  const pathname = usePathname()
  const isActive = pathname === href || !!pathname?.startsWith(href + '/')

  return (
    <Link
      href={href}
      className={cn(
        'flex items-center gap-2.5 rounded-lg text-sm transition-all duration-150',
        indent ? 'py-1.5 pl-7 pr-3' : 'py-2 px-3',
        isActive
          ? 'bg-[#6c63ff]/10 text-[#a78bfa] font-medium'
          : 'text-white/45 hover:text-white hover:bg-white/5'
      )}
    >
      <Icon className="w-3.5 h-3.5 shrink-0" />
      {label}
    </Link>
  )
}

// ─── NavGroup ─────────────────────────────────────────────────────────────────

function NavGroup({
  label,
  accent,
  defaultOpen = true,
  children,
}: {
  label: string
  accent?: string   // tailwind text-* class for the label dot
  defaultOpen?: boolean
  children: React.ReactNode
}) {
  const [open, setOpen] = useState(defaultOpen)

  return (
    <div className="pt-3">
      <button
        onClick={() => setOpen(!open)}
        className="flex items-center gap-1.5 w-full px-2 mb-1 group"
      >
        {accent && <span className={cn('w-1 h-1 rounded-full shrink-0', accent)} />}
        <span className="text-[10px] font-semibold uppercase tracking-widest text-sidebar-foreground/30 group-hover:text-sidebar-foreground/50 transition-colors flex-1 text-left">
          {label}
        </span>
        <ChevronDown className={cn(
          'w-3 h-3 text-sidebar-foreground/20 transition-transform duration-200 group-hover:text-sidebar-foreground/40',
          !open && '-rotate-90'
        )} />
      </button>
      {open && <div className="space-y-0.5">{children}</div>}
    </div>
  )
}

// ─── Sidebar ──────────────────────────────────────────────────────────────────

export function Sidebar() {
  const { user, signOut } = useAuth()

  const initials = (user?.username ?? user?.email ?? 'User')
    .split(/[\s@]+/)
    .map((n) => n[0])
    .join('')
    .toUpperCase()
    .slice(0, 2) || '?'

  return (
    <aside className="flex flex-col w-56 shrink-0 h-screen border-r bg-[hsl(var(--sidebar-background))] border-[hsl(var(--sidebar-border))]">
      {/* Logo */}
      <div className="flex items-center px-4 py-4 border-b border-[hsl(var(--sidebar-border))]">
        <FluxLogo iconSize={22} fontSize={12} gap={7} baseColor="rgba(255,255,255,0.9)" />
      </div>

      {/* CMD+K search trigger */}
      <div className="px-2 pt-3">
        <CommandPalette />
        <button
          onClick={() => {
            const e = new KeyboardEvent('keydown', { key: 'k', metaKey: true, bubbles: true })
            window.dispatchEvent(e)
          }}
          className="w-full flex items-center gap-2 px-2.5 py-1.5 rounded-lg text-xs text-muted-foreground hover:bg-muted/20 hover:text-foreground transition-colors border border-border/50 bg-muted/5"
        >
          <Search className="w-3.5 h-3.5 shrink-0" />
          <span className="flex-1 text-left">Jump to…</span>
          <kbd className="text-[10px] border border-border/60 rounded px-1 py-0.5 font-mono opacity-60">⌘K</kbd>
        </button>
      </div>

      {/* Nav */}
      <nav className="flex-1 overflow-y-auto px-2 pb-4">

        {/* Overview */}
        <div className="pt-3 space-y-0.5">
          <NavItem href="/dashboard" Icon={LayoutDashboard} label="Overview" />
        </div>

        {/* Runtime */}
        <NavGroup label="Runtime" accent="bg-emerald-400">
          <NavItem href="/dashboard/functions"  Icon={Code2}       label="Functions"  indent />
          <NavItem href="/dashboard/routes"     Icon={Globe}       label="Routes"     indent />
          <NavItem href="/dashboard/events"     Icon={Bell}        label="Events"     indent />
          <NavItem href="/dashboard/workflows"  Icon={GitBranch}   label="Workflows"  indent />
          <NavItem href="/dashboard/cron"       Icon={Clock}       label="Cron"       indent />
          <NavItem href="/dashboard/queue"      Icon={ListChecks}  label="Queues"     indent />
        </NavGroup>

        {/* Data */}
        <NavGroup label="Data" accent="bg-blue-400">
          <NavItem href="/dashboard/data"    Icon={Database}  label="Tables"          indent />
          <NavItem href="/dashboard/query"   Icon={Terminal}  label="Query Explorer"  indent />
          <NavItem href="/dashboard/schema"  Icon={Share2}    label="Schema Graph"    indent />
        </NavGroup>

        {/* Integrations */}
        <NavGroup label="Integrations" accent="bg-amber-400" defaultOpen={true}>
          <NavItem href="/dashboard/integrations" Icon={Puzzle} label="Integrations" />
        </NavGroup>

        {/* Security */}
        <NavGroup label="Security" accent="bg-rose-400">
          <NavItem href="/dashboard/secrets"   Icon={ShieldCheck} label="Secrets"   indent />
          <NavItem href="/dashboard/api-keys"  Icon={KeyRound}    label="API Keys"  indent />
        </NavGroup>

        {/* Observability */}
        <NavGroup label="Observability" accent="bg-[#a78bfa]">
          <NavItem href="/dashboard/logs"     Icon={ScrollText} label="Logs"        indent />
          <NavItem href="/dashboard/traces"   Icon={Activity}   label="Traces"      indent />
          <NavItem href="/dashboard/monitor"  Icon={BarChart2}  label="Monitor"     indent />
          <NavItem href="/dashboard/agents"   Icon={Brain}      label="Agent Runs"  indent />
          <NavItem href="/dashboard/topology" Icon={Network}    label="Topology"    indent />
        </NavGroup>

        {/* Settings */}
        <div className="pt-3 space-y-0.5">
          <NavItem href="/dashboard/settings" Icon={Settings} label="Settings" />
        </div>
      </nav>

      {/* User footer */}
      <div className="px-2 pb-3 pt-2 border-t border-[hsl(var(--sidebar-border))] mt-auto">
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <button className="flex items-center gap-2.5 w-full px-2 py-2 rounded-lg hover:bg-white/5 transition-colors text-left">
              <Avatar className="w-7 h-7 shrink-0">
                <AvatarFallback className="text-xs">{initials}</AvatarFallback>
              </Avatar>
              <div className="flex-1 min-w-0">
                <p className="text-xs font-medium text-sidebar-foreground truncate">
                  {user?.username ?? 'User'}
                </p>
                <p className="text-[10px] text-sidebar-foreground/50 truncate">{user?.email}</p>
              </div>
            </button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="start" side="top" className="w-48">
            <DropdownMenuSeparator />
            <DropdownMenuItem
              onClick={() => signOut()}
              className="text-destructive focus:text-destructive"
            >
              Sign out
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </div>
    </aside>
  )
}
