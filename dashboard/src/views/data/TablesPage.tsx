'use client'

import Link from 'next/link'
import { useQuery } from '@tanstack/react-query'
import { useParams, useRouter } from 'next/navigation'
import { Table2, ChevronRight, FileText, Cpu } from 'lucide-react'
import { cn } from '@/lib/utils'
import { apiFetch } from '@/lib/api'

interface ColumnDef {
  name: string
  type: string
  fb_type: string
  not_null?: boolean
  primary_key?: boolean
}

interface TableInfo { name: string; columns: ColumnDef[] }
interface TablesResponse { database: string; tables: TableInfo[] }

const FB_TYPE_META: Record<string, { label: string; className: string }> = {
  default:  { label: 'primitive', className: 'bg-sky-500/10 text-sky-600 dark:text-sky-400' },
  file:     { label: 'file',      className: 'bg-amber-500/10 text-amber-600 dark:text-amber-400' },
  computed: { label: 'computed',  className: 'bg-purple-500/10 text-purple-600 dark:text-purple-400' },
  relation: { label: 'relation',  className: 'bg-emerald-500/10 text-emerald-600 dark:text-emerald-400' },
}

export default function TablesPage() {
  const { database } = useParams() as any
  const router = useRouter()
  const isFlux = database !== 'public'

  const { data, isLoading } = useQuery({
    queryKey: ['tables', database],
    queryFn: () => apiFetch<TablesResponse>(`/db/tables/${database}`),
    enabled: !!database,
  })

  const tables = data?.tables ?? []

  return (
    <div className="p-8 max-w-5xl mx-auto">
      <div className="flex items-center justify-between mb-8">
        <div>
          <h1 className="text-2xl font-bold">Tables</h1>
          <p className="text-sm text-muted-foreground mt-0.5">
            {tables.length} {tables.length === 1 ? 'table' : 'tables'} in <span className="font-medium text-foreground">{database}</span> schema
            {isFlux && <span className="ml-2 text-[11px] text-amber-500/80">· read-only</span>}
          </p>
        </div>
        <div className="flex items-center rounded-lg border bg-muted/30 p-0.5">
          <Link
            href="/dashboard/data/public"
            className={cn('px-3 py-1.5 rounded-md text-xs font-medium transition-colors', !isFlux ? 'bg-background text-foreground shadow-sm' : 'text-muted-foreground hover:text-foreground')}
          >
            public
          </Link>
          <Link
            href="/dashboard/data/flux"
            className={cn('px-3 py-1.5 rounded-md text-xs font-medium transition-colors', isFlux ? 'bg-background text-foreground shadow-sm' : 'text-muted-foreground hover:text-foreground')}
          >
            flux
          </Link>
        </div>
      </div>

      {isLoading ? (
        <div className="space-y-2">
          {[...Array(4)].map((_, i) => (
            <div key={i} className="h-16 rounded-xl border bg-card animate-pulse" />
          ))}
        </div>
      ) : tables.length === 0 ? (
        <div className="flex flex-col items-center justify-center py-20 text-center border rounded-xl bg-card">
          <Table2 className="w-10 h-10 text-muted-foreground/40 mb-3" />
          <p className="font-medium text-sm">No tables yet</p>
          <p className="text-xs text-muted-foreground mt-1 text-balance">
            Define tables in your <code className="font-mono text-xs bg-muted px-1 py-0.5 rounded">schemas/</code> directory and run <code className="font-mono text-xs bg-muted px-1 py-0.5 rounded">flux db push</code>
          </p>
        </div>
      ) : (
        <div className="rounded-xl border divide-y overflow-hidden bg-card">
          {tables.map((t) => {
            const cols = Array.isArray(t.columns) ? t.columns : []
            const fileCols     = cols.filter((c) => c?.fb_type === 'file').length
            const computedCols = cols.filter((c) => c?.fb_type === 'computed').length
            return (
              <div
                key={t.name}
                className="group flex items-center justify-between px-5 py-4 hover:bg-muted/30 cursor-pointer transition-colors"
                onClick={() => router.push(`/dashboard/data/${database}/${t.name}`)}
              >
                <div className="flex items-center gap-3">
                  <div className="flex items-center justify-center w-8 h-8 rounded-lg bg-muted">
                    <Table2 className="w-4 h-4 text-muted-foreground" />
                  </div>
                  <div>
                    <p className="font-medium text-sm">{t.name}</p>
                    <p className="text-[11px] text-muted-foreground mt-0.5">
                      {cols.length} column{cols.length !== 1 ? 's' : ''}
                    </p>
                  </div>
                </div>
                <div className="flex items-center gap-2">
                  {fileCols > 0 && (
                    <span className={`inline-flex items-center gap-1 text-[10px] font-medium px-2 py-0.5 rounded-full ${FB_TYPE_META.file.className}`}>
                      <FileText className="w-2.5 h-2.5" /> {fileCols} file
                    </span>
                  )}
                  {computedCols > 0 && (
                    <span className={`inline-flex items-center gap-1 text-[10px] font-medium px-2 py-0.5 rounded-full ${FB_TYPE_META.computed.className}`}>
                      <Cpu className="w-2.5 h-2.5" /> {computedCols} computed
                    </span>
                  )}
                  <ChevronRight className="w-4 h-4 text-muted-foreground opacity-0 group-hover:opacity-100 transition-opacity" />
                </div>
              </div>
            )
          })}
        </div>
      )}
    </div>
  )
}

