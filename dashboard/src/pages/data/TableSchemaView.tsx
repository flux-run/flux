import { useMemo } from 'react'
import { useQuery } from '@tanstack/react-query'
import { useParams } from 'react-router-dom'
import { FileText, Cpu, Link as LinkIcon, AlertCircle } from 'lucide-react'
import { apiFetch } from '@/lib/api'
import { Badge } from '@/components/ui/badge'

interface SchemaColumn {
  schema: string
  table: string
  column: string
  pg_type: string
  fb_type: string
  computed_expr: string | null
  file_visibility: string | null
}

interface SchemaResponse {
  tables: Array<{ schema: string; table: string; description: string }>
  columns: SchemaColumn[]
  relationships: unknown[]
  policies: unknown[]
}

interface Props { database: string; table: string }

const FB_META: Record<string, { label: string; icon: React.ReactNode; className: string }> = {
  default:  { label: 'primitive', icon: null,                                       className: 'bg-sky-500/10 text-sky-700 dark:text-sky-400' },
  file:     { label: 'file',      icon: <FileText className="w-3 h-3" />,           className: 'bg-amber-500/10 text-amber-700 dark:text-amber-400' },
  computed: { label: 'computed',  icon: <Cpu className="w-3 h-3" />,               className: 'bg-purple-500/10 text-purple-700 dark:text-purple-400' },
  relation: { label: 'relation',  icon: <LinkIcon className="w-3 h-3" />,          className: 'bg-emerald-500/10 text-emerald-700 dark:text-emerald-400' },
}

export default function TableSchemaView({ database, table }: Props) {
  const { projectId } = useParams<{ projectId: string }>()

  const { data, isLoading, error } = useQuery({
    queryKey: ['schema', projectId, database],
    queryFn: () => apiFetch<SchemaResponse>(`/db/schema?database=${database}`),
    enabled: !!database,
  })

  const columns = useMemo(
    () => (data?.columns ?? []).filter((c) => c.table === table),
    [data, table],
  )

  if (error) {
    return (
      <div className="flex flex-col items-center justify-center py-20 gap-2 text-muted-foreground">
        <AlertCircle className="w-6 h-6 text-destructive" />
        <p className="text-sm">{String((error as Error).message)}</p>
      </div>
    )
  }

  return (
    <div className="p-6 max-w-4xl mx-auto">
      <div className="mb-4">
        <h2 className="text-sm font-semibold text-muted-foreground uppercase tracking-widest">
          Columns — {table}
        </h2>
        <p className="text-xs text-muted-foreground mt-0.5">
          {columns.length} column{columns.length !== 1 ? 's' : ''}
        </p>
      </div>

      {isLoading ? (
        <div className="rounded-xl border overflow-hidden">
          {[...Array(5)].map((_, i) => (
            <div key={i} className="flex items-center gap-4 px-5 py-4 border-b last:border-0">
              <div className="h-3 w-32 rounded bg-muted/50 animate-pulse" />
              <div className="h-3 w-20 rounded bg-muted/50 animate-pulse" />
            </div>
          ))}
        </div>
      ) : (
        <div className="rounded-xl border overflow-hidden bg-card">
          <div className="grid grid-cols-[1fr_120px_110px_1fr] gap-4 px-5 py-2.5 bg-muted/30 border-b">
            {['Column', 'Type', 'Kind', 'Details'].map((h) => (
              <p key={h} className="text-[10px] font-semibold uppercase tracking-widest text-muted-foreground/70">
                {h}
              </p>
            ))}
          </div>
          {columns.map((col) => {
            const meta = FB_META[col.fb_type] ?? FB_META.default
            return (
              <div
                key={col.column}
                className="grid grid-cols-[1fr_120px_110px_1fr] gap-4 px-5 py-3.5 border-b last:border-0 items-center hover:bg-muted/20 transition-colors"
              >
                {/* Column name */}
                <div className="flex items-center gap-2">
                  <span className="font-mono text-sm font-medium">{col.column}</span>
                </div>

                {/* PG type */}
                <span className="font-mono text-xs text-muted-foreground">{col.pg_type}</span>

                {/* fb_type badge */}
                <span className={`inline-flex items-center gap-1 text-[10px] font-medium px-2 py-0.5 rounded-full w-fit ${meta.className}`}>
                  {meta.icon}
                  {meta.label}
                </span>

                {/* Extra details */}
                <div className="text-xs text-muted-foreground space-x-2">
                  {col.fb_type === 'computed' && col.computed_expr && (
                    <span className="font-mono text-purple-500 dark:text-purple-400">
                      expr: {col.computed_expr}
                    </span>
                  )}
                  {col.fb_type === 'file' && col.file_visibility && (
                    <Badge variant="secondary" className="text-[10px]">
                      {col.file_visibility}
                    </Badge>
                  )}
                </div>
              </div>
            )
          })}
        </div>
      )}
    </div>
  )
}
