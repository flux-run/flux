'use client'

import { useState } from 'react'
import Link from 'next/link'
import { useParams, usePathname } from 'next/navigation'
import { cn } from '@/lib/utils'
import {
  FolderOpen, Settings, LayoutDashboard,
  Code2, KeyRound, ShieldCheck, ScrollText, Globe,
  Database, HardDrive, Bell, GitBranch, Clock, Terminal, Share2, Puzzle,
  Activity, Network, Brain, ChevronDown,
} from 'lucide-react'
import { FluxLogo } from '@/components/FluxLogo'
import { TenantSwitcher } from '@/components/TenantSwitcher'
import { useAuth } from '@/hooks/useAuth'
import { Avatar, AvatarFallback, AvatarImage } from '@/components/ui/avatar'
import {
  DropdownMenu, DropdownMenuContent, DropdownMenuItem,
  DropdownMenuSeparator, DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { useStore } from '@/state/tenantStore'

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
  const params = useParams() as any
  const { projectId: storeProjectId } = useStore()
  const projectId = params?.projectId ?? storeProjectId
  const { user, signOut } = useAuth()

  const initials = user?.displayName
    ?.split(' ')
    .map((n) => n[0])
    .join('')
    .toUpperCase()
    .slice(0, 2) ?? '?'

  const p = (seg: string) => `/dashboard/projects/${projectId}/${seg}`

  return (
    <aside className="flex flex-col w-56 shrink-0 h-screen border-r bg-[hsl(var(--sidebar-background))] border-[hsl(var(--sidebar-border))]">
      {/* Logo */}
      <div className="flex items-center px-4 py-4 border-b border-[hsl(var(--sidebar-border))]">
        <FluxLogo iconSize={22} fontSize={12} gap={7} baseColor="rgba(255,255,255,0.9)" />
      </div>

      {/* Tenant switcher */}
      <div className="px-2 pt-3">
        <TenantSwitcher />
      </div>

      {/* Nav */}
      <nav className="flex-1 overflow-y-auto px-2 pb-4">

        {/* Workspace */}
        <NavGroup label="Workspace">
          <NavItem href="/dashboard"        Icon={FolderOpen} label="Projects" />
          <NavItem href="/dashboard/tenants" Icon={Settings}   label="Tenant Settings" />
        </NavGroup>

        {projectId && (
          <>
            {/* Overview — top-level, no group */}
            <div className="pt-3 space-y-0.5">
              <NavItem href={p('overview')} Icon={LayoutDashboard} label="Overview" />
            </div>

            {/* Runtime */}
            <NavGroup label="Runtime" accent="bg-emerald-400">
              <NavItem href={p('functions')}  Icon={Code2}      label="Functions"  indent />
              <NavItem href={p('routes')}     Icon={Globe}      label="Routes"     indent />
              <NavItem href={p('events')}     Icon={Bell}       label="Events"     indent />
              <NavItem href={p('workflows')}  Icon={GitBranch}  label="Workflows"  indent />
              <NavItem href={p('cron')}       Icon={Clock}      label="Cron"       indent />
            </NavGroup>

            {/* Data */}
            <NavGroup label="Data" accent="bg-blue-400">
              <NavItem href={p('data')}    Icon={Database}  label="Tables"          indent />
              <NavItem href={p('storage')} Icon={HardDrive} label="Storage"         indent />
              <NavItem href={p('query')}   Icon={Terminal}  label="Query Explorer"  indent />
              <NavItem href={p('schema')}  Icon={Share2}    label="Schema Graph"    indent />
            </NavGroup>

            {/* Integrations — single item, no indent needed */}
            <NavGroup label="Integrations" accent="bg-amber-400" defaultOpen={true}>
              <NavItem href={p('integrations')} Icon={Puzzle} label="Integrations" />
            </NavGroup>

            {/* Security */}
            <NavGroup label="Security" accent="bg-rose-400">
              <NavItem href={p('secrets')}   Icon={ShieldCheck} label="Secrets"   indent />
              <NavItem href={p('api-keys')}  Icon={KeyRound}    label="API Keys"  indent />
            </NavGroup>

            {/* Observability */}
            <NavGroup label="Observability" accent="bg-[#a78bfa]">
              <NavItem href={p('logs')}     Icon={ScrollText} label="Logs"        indent />
              <NavItem href={p('traces')}   Icon={Activity}   label="Traces"      indent />
              <NavItem href={p('agents')}   Icon={Brain}      label="Agent Runs"  indent />
              <NavItem href={p('topology')} Icon={Network}    label="Topology"    indent />
            </NavGroup>

            {/* Settings — bottom standalone */}
            <div className="pt-3 space-y-0.5">
              <NavItem href={p('settings')} Icon={Settings} label="Settings" />
            </div>
          </>
        )}
      </nav>

      {/* User footer */}
      <div className="px-2 pb-3 pt-2 border-t border-[hsl(var(--sidebar-border))] mt-auto">
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <button className="flex items-center gap-2.5 w-full px-2 py-2 rounded-lg hover:bg-white/5 transition-colors text-left">
              <Avatar className="w-7 h-7 shrink-0">
                <AvatarImage src={user?.photoURL ?? ''} />
                <AvatarFallback className="text-xs">{initials}</AvatarFallback>
              </Avatar>
              <div className="flex-1 min-w-0">
                <p className="text-xs font-medium text-sidebar-foreground truncate">
                  {user?.displayName ?? 'User'}
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
