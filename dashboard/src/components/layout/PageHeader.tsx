'use client'

import Link from 'next/link'
import { ChevronRight } from 'lucide-react'
import { cn } from '@/lib/utils'

interface Breadcrumb {
  label: string
  href?: string
}

interface PageHeaderProps {
  title: string
  description?: string
  breadcrumbs?: Breadcrumb[]
  badge?: React.ReactNode
  actions?: React.ReactNode
  className?: string
}

export function PageHeader({ title, description, breadcrumbs, badge, actions, className }: PageHeaderProps) {
  return (
    <div className={cn('shrink-0 border-b bg-background/95 backdrop-blur-sm', className)}>
      {breadcrumbs && breadcrumbs.length > 0 && (
        <div className="flex items-center gap-1 px-6 pt-4 pb-0">
          {breadcrumbs.map((crumb, i) => (
            <span key={i} className="flex items-center gap-1">
              {i > 0 && <ChevronRight className="w-3 h-3 text-muted-foreground/30 shrink-0" />}
              {crumb.href ? (
                <Link
                  href={crumb.href}
                  className="text-xs text-muted-foreground hover:text-foreground transition-colors"
                >
                  {crumb.label}
                </Link>
              ) : (
                <span className="text-xs text-muted-foreground/60">{crumb.label}</span>
              )}
            </span>
          ))}
        </div>
      )}
      <div className="flex items-center justify-between gap-4 px-6 py-4">
        <div className="min-w-0">
          <div className="flex items-center gap-2">
            <h1 className="text-lg font-semibold tracking-tight truncate">{title}</h1>
            {badge}
          </div>
          {description && (
            <p className="text-sm text-muted-foreground mt-0.5 truncate">{description}</p>
          )}
        </div>
        {actions && (
          <div className="flex items-center gap-2 shrink-0">{actions}</div>
        )}
      </div>
    </div>
  )
}
