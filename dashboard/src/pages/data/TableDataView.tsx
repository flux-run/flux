import { useState, useMemo } from 'react'
import { useQuery } from '@tanstack/react-query'
import { useParams } from 'react-router-dom'
import {
  createColumnHelper,
  flexRender,
  getCoreRowModel,
  useReactTable,
} from '@tanstack/react-table'
import { FileText, Cpu, Link as LinkIcon, RefreshCw, AlertCircle } from 'lucide-react'
import { apiFetch, gatewayFetch } from '@/lib/api'
import { Button } from '@/components/ui/button'

interface SchemaColumn {
  schema: string
  table: string
  column: string
  pg_type: string
  fb_type: string
}

interface SchemaResponse {
  columns: SchemaColumn[]
}

interface TableDataViewProps {
  database: string
  table: string
}

const FB_ICON: Record<string, React.ReactNode> = {
  file:     <FileText className="w-3 h-3 text-amber-500" />,
  computed: <Cpu      className="w-3 h-3 text-purple-500" />,
  relation: <LinkIcon className="w-3 h-3 text-emerald-500" />,
}

const FB_CLASS: Record<string, string> = {
  file:     'text-amber-600 dark:text-amber-400',
  computed: 'text-purple-600 dark:text-purple-400',
  relation: 'text-emerald-600 dark:text-emerald-400',
}

export default function TableDataView({ database, table }: TableDataViewProps) {
  const { projectId } = useParams<{ projectId: string }>()
  const [limit] = useState(100)

  // Schema (for column type annotations)
  const schemaQ = useQuery({
    queryKey: ['schema-cols', projectId, database, table],
    queryFn: () => apiFetch<SchemaResponse>(`/db/schema?database=${database}`),
    enabled: !!database,
  })

  const colMeta = useMemo(() => {
    const map: Record<string, SchemaColumn> = {}
    for (const c of schemaQ.data?.columns ?? []) {
      if (c.table === table) map[c.column] = c
    }
    return map
  }, [schemaQ.data, table])

  // Actual data
  const dataQ = useQuery({
    queryKey: ['table-data', projectId, database, table, limit],
    queryFn: () =>
      gatewayFetch<unknown[]>('/db/query', {
        method: 'POST',
        body: JSON.stringify({ database, table, operation: 'select', limit }),
      }),
    enabled: !!database && !!table,
  })

  const rows: Record<string, unknown>[] = useMemo(() => {
    const raw = dataQ.data
    if (!raw) return []
    if (Array.isArray(raw)) return raw as Record<string, unknown>[]
    if (typeof raw === 'object' && 'data' in raw && Array.isArray((raw as { data: unknown }).data))
      return (raw as { data: unknown[] }).data as Record<string, unknown>[]
    return []
  }, [dataQ.data])

  // Build TanStack columns dynamically from first row
  const columnHelper = createColumnHelper<Record<string, unknown>>()

  const tanstackCols = useMemo(() => {
    if (rows.length === 0) return []
    return Object.keys(rows[0]).map((key) =>
      columnHelper.accessor(key, {
        id: key,
        header: key,
        cell: (info) => {
          const val = info.getValue()
          const meta = colMeta[key]
          if (meta?.fb_type === 'file' && typeof val === 'string') {
            return (
              <span className="flex items-center gap-1 text-amber-600 dark:text-amber-400">
                <FileText className="w-3 h-3 shrink-0" />
                <span className="truncate max-w-[160px]">{val}</span>
              </span>
            )
          }
          if (val === null || val === undefined) {
            return <span className="text-muted-foreground/40 text-[11px]">null</span>
          }
          if (typeof val === 'object') {
            return <span className="font-mono text-[11px] text-muted-foreground">{JSON.stringify(val)}</span>
          }
          return <span>{String(val)}</span>
        },
      }),
    )
  }, [rows, colMeta])

  const reactTable = useReactTable({
    data: rows,
    columns: tanstackCols,
    getCoreRowModel: getCoreRowModel(),
  })

  const isLoading = dataQ.isLoading || schemaQ.isLoading

  return (
    <div className="flex flex-col h-full">
      {/* Toolbar */}
      <div className="flex items-center justify-between px-6 py-3 border-b bg-muted/20">
        <p className="text-xs text-muted-foreground">
          {rows.length > 0 ? `${rows.length} row${rows.length !== 1 ? 's' : ''}` : 'No rows'}
        </p>
        <Button
          variant="ghost" size="sm" className="h-7 text-xs gap-1"
          onClick={() => dataQ.refetch()}
          disabled={dataQ.isFetching}
        >
          <RefreshCw className={`w-3 h-3 ${dataQ.isFetching ? 'animate-spin' : ''}`} />
          Refresh
        </Button>
      </div>

      {dataQ.error ? (
        <div className="flex flex-col items-center justify-center flex-1 gap-2 text-muted-foreground">
          <AlertCircle className="w-6 h-6 text-destructive" />
          <p className="text-sm">{String((dataQ.error as Error).message)}</p>
        </div>
      ) : isLoading ? (
        <div className="flex-1 overflow-auto">
          <table className="w-full text-sm">
            <tbody>
              {[...Array(6)].map((_, i) => (
                <tr key={i} className="border-b">
                  {[...Array(4)].map((_, j) => (
                    <td key={j} className="px-4 py-3">
                      <div className="h-3 rounded bg-muted/50 animate-pulse w-24" />
                    </td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      ) : rows.length === 0 ? (
        <div className="flex flex-col items-center justify-center flex-1 gap-2 text-muted-foreground">
          <p className="text-sm">This table has no rows yet.</p>
        </div>
      ) : (
        <div className="flex-1 overflow-auto">
          <table className="w-full text-xs border-collapse">
            <thead className="sticky top-0 z-10 bg-muted/80 backdrop-blur">
              {reactTable.getHeaderGroups().map((hg) => (
                <tr key={hg.id}>
                  {hg.headers.map((h) => {
                    const meta = colMeta[h.id]
                    const fbType = meta?.fb_type ?? 'default'
                    return (
                      <th
                        key={h.id}
                        className="px-4 py-2.5 text-left font-semibold text-muted-foreground border-b border-r last:border-r-0 whitespace-nowrap"
                      >
                        <div className="flex items-center gap-1.5">
                          {FB_ICON[fbType]}
                          <span className={FB_CLASS[fbType] ?? ''}>{h.id}</span>
                          {meta && (
                            <span className="text-[9px] text-muted-foreground/50 font-normal">
                              {meta.pg_type}
                            </span>
                          )}
                        </div>
                      </th>
                    )
                  })}
                </tr>
              ))}
            </thead>
            <tbody>
              {reactTable.getRowModel().rows.map((row, i) => (
                <tr
                  key={row.id}
                  className={`hover:bg-muted/20 transition-colors ${i % 2 === 0 ? '' : 'bg-muted/5'}`}
                >
                  {row.getVisibleCells().map((cell) => (
                    <td
                      key={cell.id}
                      className="px-4 py-2.5 border-b border-r last:border-r-0 font-mono max-w-[240px] truncate"
                    >
                      {flexRender(cell.column.columnDef.cell, cell.getContext())}
                    </td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  )
}
