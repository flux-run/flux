import { NavLink, useParams } from 'react-router-dom'
import { cn } from '@/lib/utils'
import { Zap, FolderOpen, Settings, LayoutDashboard,
  Code2, KeyRound, ShieldCheck, ScrollText, Globe,
  Database, HardDrive, Bell, GitBranch, Clock, Terminal, Share2,
} from 'lucide-react'
import { TenantSwitcher } from '@/components/TenantSwitcher'
import { useAuth } from '@/hooks/useAuth'
import { Avatar, AvatarFallback, AvatarImage } from '@/components/ui/avatar'
import {
  DropdownMenu, DropdownMenuContent, DropdownMenuItem,
  DropdownMenuSeparator, DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { useStore } from '@/state/tenantStore'

const navItem = (
  to: string,
  Icon: React.ComponentType<{ className?: string }>,
  label: string
) => (
  <NavLink
    to={to}
    className={({ isActive }) =>
      cn(
        'flex items-center gap-2.5 px-3 py-2 rounded-lg text-sm transition-all duration-150',
        isActive
          ? 'bg-primary/15 text-primary font-medium'
          : 'text-sidebar-foreground/60 hover:text-sidebar-foreground hover:bg-white/5'
      )
    }
  >
    <Icon className="w-4 h-4 shrink-0" />
    {label}
  </NavLink>
)

export function Sidebar() {
  const { projectId: paramProjectId } = useParams<{ projectId?: string }>()
  const { projectId: storeProjectId } = useStore()
  const projectId = paramProjectId ?? storeProjectId
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
        <div className="flex items-center justify-center w-7 h-7 rounded-lg bg-primary/20">
          <Zap className="w-4 h-4 text-primary" />
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
          {navItem('/dashboard', FolderOpen, 'Projects')}
          {navItem('/dashboard/tenants', Settings, 'Tenant Settings')}
        </div>

        {projectId && (
          <div className="px-1 pt-3">
            <p className="text-[10px] font-semibold uppercase tracking-widest text-sidebar-foreground/30 mb-1.5 px-2">
              Project
            </p>
            {navItem(`/dashboard/projects/${projectId}/overview`,  LayoutDashboard, 'Overview')}
            {navItem(`/dashboard/projects/${projectId}/data`,       Database,        'Data')}
            {navItem(`/dashboard/projects/${projectId}/storage`,    HardDrive,       'Storage')}
            {navItem(`/dashboard/projects/${projectId}/query`,      Terminal,        'Query Explorer')}
            {navItem(`/dashboard/projects/${projectId}/schema`,      Share2,          'Schema Graph')}
            {navItem(`/dashboard/projects/${projectId}/functions`,  Code2,           'Functions')}
            {navItem(`/dashboard/projects/${projectId}/routes`,     Globe,           'Routes')}
            {navItem(`/dashboard/projects/${projectId}/events`,     Bell,            'Events')}
            {navItem(`/dashboard/projects/${projectId}/workflows`,  GitBranch,       'Workflows')}
            {navItem(`/dashboard/projects/${projectId}/cron`,       Clock,           'Cron')}
            {navItem(`/dashboard/projects/${projectId}/secrets`,    ShieldCheck,     'Secrets')}
            {navItem(`/dashboard/projects/${projectId}/api-keys`,   KeyRound,        'API Keys')}
            {navItem(`/dashboard/projects/${projectId}/logs`,       ScrollText,      'Logs')}
            {navItem(`/dashboard/projects/${projectId}/settings`,   Settings,        'Settings')}
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
