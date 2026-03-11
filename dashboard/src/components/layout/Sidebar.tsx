'use client'

import Link from 'next/link'
import { useParams, usePathname } from 'next/navigation'
import { cn } from '@/lib/utils'
import { Zap, FolderOpen, Settings, LayoutDashboard,
  Code2, KeyRound, ShieldCheck, ScrollText, Globe,
  Database, HardDrive, Bell, GitBranch, Clock, Terminal, Share2, Puzzle,
} from 'lucide-react'
import { TenantSwitcher } from '@/components/TenantSwitcher'
import { useAuth } from '@/hooks/useAuth'
import { Avatar, AvatarFallback, AvatarImage } from '@/components/ui/avatar'
import {
  DropdownMenu, DropdownMenuContent, DropdownMenuItem,
  DropdownMenuSeparator, DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { useStore } from '@/state/tenantStore'

function NavItem({
  href,
  Icon,
  label,
}: {
  href: string
  Icon: React.ComponentType<{ className?: string }>
  label: string
}) {
  const pathname = usePathname()
  const isActive = pathname === href || !!pathname?.startsWith(href + '/')

  return (
    <Link
      href={href}
      className={cn(
        'flex items-center gap-2.5 px-3 py-2 rounded-lg text-sm transition-all duration-150',
        isActive
          ? 'bg-white/10 text-white font-medium'
          : 'text-white/50 hover:text-white hover:bg-white/5'
      )}
    >
      <Icon className="w-4 h-4 shrink-0" />
      {label}
    </Link>
  )
}

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

  return (
    <aside className="flex flex-col w-60 shrink-0 h-screen border-r bg-[hsl(var(--sidebar-background))] border-[hsl(var(--sidebar-border))]">
      {/* Logo */}
      <div className="flex items-center gap-2.5 px-4 py-4 border-b border-[hsl(var(--sidebar-border))]">
        <div className="flex items-center justify-center w-7 h-7 rounded-lg bg-white/10">
          <Zap className="w-4 h-4 text-white" />
        </div>
        <span className="font-bold text-sm tracking-tight text-sidebar-foreground">Fluxbase</span>
      </div>

      {/* Tenant switcher */}
      <div className="px-2 pt-3">
        <TenantSwitcher />
      </div>

      {/* Main nav */}
      <nav className="flex-1 overflow-y-auto px-2 pt-3 space-y-0.5">
        <div className="px-1 pb-1">
          <p className="text-[10px] font-semibold uppercase tracking-widest text-sidebar-foreground/30 mb-1.5 px-2">
            Workspace
          </p>
          <NavItem href="/dashboard" Icon={FolderOpen} label="Projects" />
          <NavItem href="/dashboard/tenants" Icon={Settings} label="Tenant Settings" />
        </div>

        {projectId && (
          <div className="px-1 pt-3">
            <p className="text-[10px] font-semibold uppercase tracking-widest text-sidebar-foreground/30 mb-1.5 px-2">
              Project
            </p>
            <NavItem href={`/dashboard/projects/${projectId}/overview`}  Icon={LayoutDashboard} label="Overview" />
            <NavItem href={`/dashboard/projects/${projectId}/data`}       Icon={Database}        label="Data" />
            <NavItem href={`/dashboard/projects/${projectId}/storage`}    Icon={HardDrive}       label="Storage" />
            <NavItem href={`/dashboard/projects/${projectId}/query`}      Icon={Terminal}        label="Query Explorer" />
            <NavItem href={`/dashboard/projects/${projectId}/schema`}     Icon={Share2}          label="Schema Graph" />
            <NavItem href={`/dashboard/projects/${projectId}/functions`}  Icon={Code2}           label="Functions" />
            <NavItem href={`/dashboard/projects/${projectId}/routes`}     Icon={Globe}           label="Routes" />
            <NavItem href={`/dashboard/projects/${projectId}/events`}     Icon={Bell}            label="Events" />
            <NavItem href={`/dashboard/projects/${projectId}/workflows`}  Icon={GitBranch}       label="Workflows" />
            <NavItem href={`/dashboard/projects/${projectId}/cron`}       Icon={Clock}           label="Cron" />
            <NavItem href={`/dashboard/projects/${projectId}/integrations`} Icon={Puzzle}        label="Integrations" />
            <NavItem href={`/dashboard/projects/${projectId}/secrets`}    Icon={ShieldCheck}     label="Secrets" />
            <NavItem href={`/dashboard/projects/${projectId}/api-keys`}   Icon={KeyRound}        label="API Keys" />
            <NavItem href={`/dashboard/projects/${projectId}/logs`}       Icon={ScrollText}      label="Logs" />
            <NavItem href={`/dashboard/projects/${projectId}/settings`}   Icon={Settings}        label="Settings" />
          </div>
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
