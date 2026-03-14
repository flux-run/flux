'use client'

import * as React from 'react'
import * as ToastPrimitives from '@radix-ui/react-toast'
import { cn } from '@/lib/utils'
import { X, CheckCircle2, AlertCircle, Info, AlertTriangle } from 'lucide-react'

// ─── Provider ────────────────────────────────────────────────────────────────

export const ToastProvider = ToastPrimitives.Provider

// ─── Types ────────────────────────────────────────────────────────────────────

export type ToastVariant = 'default' | 'success' | 'error' | 'warning' | 'info'

export interface ToastData {
  id: string
  title: string
  description?: string
  variant?: ToastVariant
  duration?: number
}

// ─── Context ──────────────────────────────────────────────────────────────────

interface ToastCtx {
  toast: (data: Omit<ToastData, 'id'>) => void
}

const Ctx = React.createContext<ToastCtx | null>(null)

export function useToast(): ToastCtx {
  const ctx = React.useContext(Ctx)
  if (!ctx) throw new Error('useToast must be used inside <ToastRoot>')
  return ctx
}

// ─── Variants ─────────────────────────────────────────────────────────────────

const VARIANT: Record<ToastVariant, { cls: string; Icon: React.ComponentType<{ className?: string }> }> = {
  default: { cls: 'border-border bg-card text-foreground',             Icon: Info          },
  success: { cls: 'border-emerald-500/40 bg-emerald-950 text-emerald-100', Icon: CheckCircle2  },
  error:   { cls: 'border-red-500/40 bg-red-950 text-red-100',             Icon: AlertCircle   },
  warning: { cls: 'border-amber-500/40 bg-amber-950 text-amber-100',       Icon: AlertTriangle },
  info:    { cls: 'border-sky-500/40 bg-sky-950 text-sky-100',             Icon: Info          },
}

// ─── Root (provider + viewport) ───────────────────────────────────────────────

export function ToastRoot({ children }: { children: React.ReactNode }) {
  const [toasts, setToasts] = React.useState<ToastData[]>([])

  const toast = React.useCallback((data: Omit<ToastData, 'id'>) => {
    const id = Math.random().toString(36).slice(2)
    setToasts(prev => [...prev, { ...data, id }])
  }, [])

  const dismiss = React.useCallback((id: string) => {
    setToasts(prev => prev.filter(t => t.id !== id))
  }, [])

  return (
    <Ctx.Provider value={{ toast }}>
      <ToastProvider swipeDirection="right">
        {children}

        {toasts.map(t => {
          const { cls, Icon } = VARIANT[t.variant ?? 'default']
          return (
            <ToastPrimitives.Root
              key={t.id}
              duration={t.duration ?? 4000}
              onOpenChange={open => { if (!open) dismiss(t.id) }}
              className={cn(
                'group pointer-events-auto relative flex w-full max-w-sm items-start gap-3 overflow-hidden rounded-xl border px-4 py-3 shadow-lg',
                'data-[state=open]:animate-in data-[state=open]:slide-in-from-right-5 data-[state=open]:fade-in',
                'data-[state=closed]:animate-out data-[state=closed]:slide-out-to-right-5 data-[state=closed]:fade-out',
                cls,
              )}
            >
              <Icon className="mt-0.5 h-4 w-4 shrink-0 opacity-80" />
              <div className="flex-1 space-y-0.5 min-w-0">
                <ToastPrimitives.Title className="text-sm font-medium leading-snug">
                  {t.title}
                </ToastPrimitives.Title>
                {t.description && (
                  <ToastPrimitives.Description className="text-xs opacity-70 leading-snug">
                    {t.description}
                  </ToastPrimitives.Description>
                )}
              </div>
              <ToastPrimitives.Close
                onClick={() => dismiss(t.id)}
                className="opacity-0 group-hover:opacity-60 hover:!opacity-100 transition-opacity shrink-0 -mr-1 -mt-0.5"
              >
                <X className="h-3.5 w-3.5" />
              </ToastPrimitives.Close>
            </ToastPrimitives.Root>
          )
        })}

        <ToastPrimitives.Viewport className="fixed bottom-4 right-4 z-[9999] flex flex-col gap-2 w-[360px] max-w-[calc(100vw-2rem)]" />
      </ToastProvider>
    </Ctx.Provider>
  )
}
