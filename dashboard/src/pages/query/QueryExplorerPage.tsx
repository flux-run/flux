import { useState, useCallback } from 'react'
import { useQuery, useMutation } from '@tanstack/react-query'
import {
  useReactTable,
  getCoreRowModel,
  flexRender,
  type ColumnDef,
} from '@tanstack/react-table'
import { dbFetch } from '@/lib/api'
import { useStore } from '@/state/tenantStore'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Badge } from '@/components/ui/badge'
import { Textarea } from '@/components/ui/textarea'
import { Tabs, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Separator } from '@/components/ui/separator'
import {
  Play, Plus, Trash2, RefreshCw, Table2, Braces,
  ChevronRight, AlertCircle,
} from 'lucide-react'
import { cn } from '@/lib/utils'

// ─── Types ────────────────────────────────────────────────────────────────────

type Operation = 'select' | 'insert' | 'update' | 'delete'

interface Filter {
  column: string
  op: string
  value: string
}

const FILTER_OPS = [
  { value: 'eq',       label: '= eq' },
  { value: 'neq',      label: '≠ neq' },
  { value: 'gt',       label: '> gt' },
  { value: 'gte',      label: '≥ gte' },
  { value: 'lt',       label: '< lt' },
  { value: 'lte',      label: '≤ lte' },
  { value: 'like',     label: '~ like' },
  { value: 'ilike',    label: '~* ilike' },
  { value: 'is_null',  label: 'IS NULL' },
  { value: 'not_null', label: 'NOT NULL' },
]

const OP_COLORS: Record<Operation, string> = {
  select: 'bg-sky-500/15 text-sky-400 border-sky-500/20',
  insert: 'bg-emerald-500/15 text-emerald-400 border-emerald-500/20',
  update: 'bg-amber-500/15 text-amber-400 border-amber-500/20',
  delete: 'bg-red-500/15 text-red-400 border-red-500/20',
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

function safeParseJson(s: string): { ok: boolean; value: unknown; error?: string } {
  try {
    return { ok: true, value: JSON.parse(s) }
  } catch (e) {
    return { ok: false, value: null, error: (e as Error).message }
  }
}

// ─── Sub-components ───────────────────────────────────────────────────────────

function FilterRow({
  filter,
  columns,
  onChange,
  onRemove,
}: {
  filter: Filter
  columns: string[]
  onChange: (f: Filter) => void
  onRemove: () => void
}) {
  const noValue = filter.op === 'is_null' || filter.op === 'not_null'
  return (
    <div className="flex items-center gap-2">
      <Select value={filter.column} onValueChange={(v) => onChange({ ...filter, column: v })}>
        <SelectTrigger className="h-7 text-xs w-36 bg-white/5 border-white/10">
          <SelectValue placeholder="column" />
        </SelectTrigger>
        <SelectContent>
          {columns.map((c) => (
            <SelectItem key={c} value={c} className="text-xs">{c}</SelectItem>
          ))}
        </SelectContent>
      </Select>

      <Select value={filter.op} onValueChange={(v) => onChange({ ...filter, op: v, value: '' })}>
        <SelectTrigger className="h-7 text-xs w-28 bg-white/5 border-white/10">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {FILTER_OPS.map((o) => (
            <SelectItem key={o.value} value={o.value} className="text-xs">{o.label}</SelectItem>
          ))}
        </SelectContent>
      </Select>

      {!noValue && (
        <Input
          value={filter.value}
          onChange={(e) => onChange({ ...filter, value: e.target.value })}
          placeholder="value"
          className="h-7 text-xs flex-1 bg-white/5 border-white/10"
        />
      )}

      <Button variant="ghost" size="icon" className="h-7 w-7 shrink-0 text-muted-foreground hover:text-destructive" onClick={onRemove}>
        <Trash2 className="w-3.5 h-3.5" />
      </Button>
    </div>
  )
}

function ResultTable({ rows }: { rows: Record<string, unknown>[] }) {
  const columns: ColumnDef<Record<string, unknown>>[] = rows.length === 0 ? [] :
    Object.keys(rows[0]).map((key) => ({
      id: key,
      accessorKey: key,
      header: key,
      cell: ({ getValue }) => {
        const v = getValue()
        if (v === null || v === undefined) return <span className="text-muted-foreground/40 text-xs italic">null</span>
        if (typeof v === 'object') return <span className="text-purple-400 text-xs font-mono">{JSON.stringify(v)}</span>
        return <span className="text-xs font-mono">{String(v)}</span>
      },
    }))

  const table = useReactTable({
    data: rows,
    columns,
    getCoreRowModel: getCoreRowModel(),
  })

  if (rows.length === 0) {
    return (
      <div className="flex-1 flex items-center justify-center text-sm text-muted-foreground">
        No rows returned.
      </div>
    )
  }

  return (
    <div className="flex-1 overflow-auto">
      <table className="w-full text-xs border-collapse">
        <thead className="sticky top-0 z-10 bg-[hsl(var(--sidebar-background))]">
          {table.getHeaderGroups().map((hg) => (
            <tr key={hg.id}>
              {hg.headers.map((h) => (
                <th
                  key={h.id}
                  className="text-left px-3 py-2 font-semibold text-muted-foreground border-b border-white/5 whitespace-nowrap"
                >
                  {flexRender(h.column.columnDef.header, h.getContext())}
                </th>
              ))}
            </tr>
          ))}
        </thead>
        <tbody>
          {table.getRowModel().rows.map((row, i) => (
            <tr key={row.id} className={cn('hover:bg-white/3', i % 2 === 0 ? '' : 'bg-white/[0.015]')}>
              {row.getVisibleCells().map((cell) => (
                <td key={cell.id} className="px-3 py-1.5 border-b border-white/5 whitespace-nowrap max-w-xs truncate">
                  {flexRender(cell.column.columnDef.cell, cell.getContext())}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  )
}

// ─── Main Page ────────────────────────────────────────────────────────────────

export default function QueryExplorerPage() {
  const { projectId } = useStore()

  /* Selectors */
  const [database, setDatabase] = useState('')
  const [table, setTable] = useState('')
  const [operation, setOperation] = useState<Operation>('select')

  /* Select-specific */
  const [selectedCols, setSelectedCols] = useState<string[]>([])
  const [filters, setFilters] = useState<Filter[]>([])
  const [limit, setLimit] = useState('100')
  const [offset, setOffset] = useState('0')

  /* Insert/Update */
  const [dataJson, setDataJson] = useState('{\n  \n}')

  /* Results */
  const [rows, setRows] = useState<Record<string, unknown>[]>([])
  const [resultView, setResultView] = useState<'table' | 'json'>('table')
  const [execMeta, setExecMeta] = useState<{ ms: number; count: number } | null>(null)
  const [execError, setExecError] = useState<string | null>(null)

  /* Data fetching */
  const { data: dbData } = useQuery({
    queryKey: ['databases', projectId],
    queryFn: () => dbFetch<{ databases: string[] }>('/db/databases'),
    enabled: !!projectId,
  })

  const { data: tablesData } = useQuery({
    queryKey: ['tables', projectId, database],
    queryFn: () => dbFetch<{ database: string; tables: { name: string; columns: { name: string }[] }[] }>(`/db/tables/${database}`),
    enabled: !!database,
  })

  const tableColumns: string[] = tablesData?.tables
    ?.find((t) => t.name === table)
    ?.columns.map((c) => c.name) ?? []

  /* Execute mutation */
  const executeMutation = useMutation({
    mutationFn: async () => {
      setExecError(null)

      // Build QueryRequest
      const req: Record<string, unknown> = {
        database,
        table,
        operation,
      }

      if (operation === 'select') {
        if (selectedCols.length > 0) req.columns = selectedCols
        const validFilters = filters.filter((f) => f.column)
        if (validFilters.length > 0) {
          req.filters = validFilters.map((f) => ({
            column: f.column,
            op: f.op,
            value: f.op === 'is_null' || f.op === 'not_null' ? null : (() => {
              const n = Number(f.value)
              return isNaN(n) || f.value === '' ? f.value : n
            })(),
          }))
        }
        if (limit) req.limit = parseInt(limit, 10)
        if (offset && offset !== '0') req.offset = parseInt(offset, 10)
      }

      if (operation === 'insert' || operation === 'update') {
        const parsed = safeParseJson(dataJson)
        if (!parsed.ok) throw new Error(`Invalid JSON: ${parsed.error}`)
        req.data = parsed.value
      }

      if (operation === 'update' || operation === 'delete') {
        const validFilters = filters.filter((f) => f.column)
        if (validFilters.length > 0) {
          req.filters = validFilters.map((f) => ({
            column: f.column,
            op: f.op,
            value: f.op === 'is_null' || f.op === 'not_null' ? null : f.value,
          }))
        }
      }

      const t0 = performance.now()
      const result = await dbFetch<unknown>('/db/query', {
        method: 'POST',
        body: JSON.stringify(req),
      })
      const ms = Math.round(performance.now() - t0)

      const resultRows = Array.isArray(result) ? result as Record<string, unknown>[] : []
      return { rows: resultRows, ms, raw: result }
    },
    onSuccess: ({ rows: r, ms }) => {
      setRows(r)
      setExecMeta({ ms, count: r.length })
    },
    onError: (err: Error) => {
      setExecError(err.message)
      setRows([])
      setExecMeta(null)
    },
  })

  const addFilter = useCallback(() => {
    setFilters((prev) => [...prev, { column: tableColumns[0] ?? '', op: 'eq', value: '' }])
  }, [tableColumns])

  const updateFilter = useCallback((i: number, f: Filter) => {
    setFilters((prev) => prev.map((item, idx) => (idx === i ? f : item)))
  }, [])

  const removeFilter = useCallback((i: number) => {
    setFilters((prev) => prev.filter((_, idx) => idx !== i))
  }, [])

  const toggleCol = (col: string) => {
    setSelectedCols((prev) =>
      prev.includes(col) ? prev.filter((c) => c !== col) : [...prev, col]
    )
  }

  const jsonParseStatus = (operation === 'insert' || operation === 'update')
    ? safeParseJson(dataJson)
    : { ok: true }

  const canExecute = !!database && !!table && !executeMutation.isPending

  return (
    <div className="flex h-full overflow-hidden">
      {/* Left panel — Query builder */}
      <aside className="w-72 shrink-0 flex flex-col border-r border-white/5 overflow-y-auto bg-background/40">
        <div className="px-4 pt-5 pb-3">
          <h2 className="text-sm font-semibold">Query Explorer</h2>
          <p className="text-xs text-muted-foreground mt-0.5">Build and run queries visually</p>
        </div>
        <Separator className="bg-white/5" />

        <div className="flex-1 px-4 py-4 space-y-5">
          {/* Database */}
          <div className="space-y-1.5">
            <Label className="text-xs text-muted-foreground">Database</Label>
            <Select value={database} onValueChange={(v) => { setDatabase(v); setTable(''); setSelectedCols([]); setFilters([]) }}>
              <SelectTrigger className="h-8 text-xs bg-white/5 border-white/10">
                <SelectValue placeholder="Select database…" />
              </SelectTrigger>
              <SelectContent>
                {(dbData?.databases ?? []).map((d) => (
                  <SelectItem key={d} value={d} className="text-xs">{d}</SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>

          {/* Table */}
          <div className="space-y-1.5">
            <Label className="text-xs text-muted-foreground">Table</Label>
            <Select value={table} onValueChange={(v) => { setTable(v); setSelectedCols([]); setFilters([]) }} disabled={!database}>
              <SelectTrigger className="h-8 text-xs bg-white/5 border-white/10">
                <SelectValue placeholder="Select table…" />
              </SelectTrigger>
              <SelectContent>
                {(tablesData?.tables ?? []).map((t) => (
                  <SelectItem key={t.name} value={t.name} className="text-xs">{t.name}</SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>

          {/* Operation */}
          <div className="space-y-1.5">
            <Label className="text-xs text-muted-foreground">Operation</Label>
            <div className="flex gap-1.5 flex-wrap">
              {(['select', 'insert', 'update', 'delete'] as Operation[]).map((op) => (
                <button
                  key={op}
                  onClick={() => { setOperation(op); setFilters([]); setSelectedCols([]) }}
                  className={cn(
                    'px-2.5 py-1 rounded text-xs font-mono font-medium border transition-all',
                    operation === op ? OP_COLORS[op] : 'bg-white/5 text-muted-foreground border-white/10 hover:border-white/20'
                  )}
                >
                  {op.toUpperCase()}
                </button>
              ))}
            </div>
          </div>

          <Separator className="bg-white/5" />

          {/* Select: columns */}
          {operation === 'select' && tableColumns.length > 0 && (
            <div className="space-y-1.5">
              <Label className="text-xs text-muted-foreground">Columns <span className="text-foreground/30">(all if none selected)</span></Label>
              <div className="space-y-1.5 max-h-40 overflow-y-auto">
                {tableColumns.map((col) => (
                  <label key={col} className="flex items-center gap-2 cursor-pointer group">
                    <input
                      type="checkbox"
                      checked={selectedCols.includes(col)}
                      onChange={() => toggleCol(col)}
                      className="rounded"
                    />
                    <span className="text-xs font-mono group-hover:text-foreground text-muted-foreground transition-colors">{col}</span>
                  </label>
                ))}
              </div>
            </div>
          )}

          {/* Insert/Update: JSON body */}
          {(operation === 'insert' || operation === 'update') && (
            <div className="space-y-1.5">
              <div className="flex items-center justify-between">
                <Label className="text-xs text-muted-foreground">Data (JSON)</Label>
                {!jsonParseStatus.ok && (
                  <span className="text-[10px] text-red-400 flex items-center gap-1">
                    <AlertCircle className="w-3 h-3" /> invalid
                  </span>
                )}
              </div>
              <Textarea
                value={dataJson}
                onChange={(e: React.ChangeEvent<HTMLTextAreaElement>) => setDataJson(e.target.value)}
                className="font-mono text-xs min-h-32 bg-white/5 border-white/10 resize-none"
                spellCheck={false}
              />
            </div>
          )}

          {/* Filters — select / update / delete */}
          {(operation === 'select' || operation === 'update' || operation === 'delete') && (
            <div className="space-y-2">
              <div className="flex items-center justify-between">
                <Label className="text-xs text-muted-foreground">Filters</Label>
                <Button
                  variant="ghost"
                  size="sm"
                  className="h-6 px-2 text-xs text-muted-foreground hover:text-foreground"
                  onClick={addFilter}
                  disabled={!table}
                >
                  <Plus className="w-3 h-3 mr-1" /> Add
                </Button>
              </div>
              <div className="space-y-2">
                {filters.map((f, i) => (
                  <FilterRow
                    key={i}
                    filter={f}
                    columns={tableColumns}
                    onChange={(updated) => updateFilter(i, updated)}
                    onRemove={() => removeFilter(i)}
                  />
                ))}
                {filters.length === 0 && (
                  <p className="text-xs text-muted-foreground/40 italic">No filters — returns all rows</p>
                )}
              </div>
            </div>
          )}

          {/* Select: limit/offset */}
          {operation === 'select' && (
            <div className="grid grid-cols-2 gap-2">
              <div className="space-y-1">
                <Label className="text-xs text-muted-foreground">Limit</Label>
                <Input
                  value={limit}
                  onChange={(e) => setLimit(e.target.value)}
                  type="number"
                  min="1"
                  max="10000"
                  className="h-7 text-xs bg-white/5 border-white/10"
                />
              </div>
              <div className="space-y-1">
                <Label className="text-xs text-muted-foreground">Offset</Label>
                <Input
                  value={offset}
                  onChange={(e) => setOffset(e.target.value)}
                  type="number"
                  min="0"
                  className="h-7 text-xs bg-white/5 border-white/10"
                />
              </div>
            </div>
          )}
        </div>

        {/* Execute button */}
        <div className="px-4 pb-4 pt-2 border-t border-white/5">
          <Button
            onClick={() => executeMutation.mutate()}
            disabled={!canExecute || (operation === 'delete' && filters.length === 0)}
            className="w-full h-8 text-xs gap-2"
          >
            {executeMutation.isPending
              ? <RefreshCw className="w-3.5 h-3.5 animate-spin" />
              : <Play className="w-3.5 h-3.5" />}
            {executeMutation.isPending ? 'Running…' : 'Run Query'}
          </Button>
          {operation === 'delete' && filters.length === 0 && (
            <p className="text-[10px] text-amber-400 mt-1.5 text-center">Add a filter to prevent full-table delete</p>
          )}
        </div>
      </aside>

      {/* Right panel — Results */}
      <main className="flex-1 flex flex-col overflow-hidden">
        {/* Result header */}
        <div className="flex items-center gap-4 px-5 py-3 border-b border-white/5 shrink-0">
          {/* Breadcrumb */}
          <div className="flex items-center gap-1.5 text-sm text-muted-foreground min-w-0">
            {database && <span className="truncate">{database}</span>}
            {database && table && <ChevronRight className="w-3.5 h-3.5 shrink-0" />}
            {table && <span className="truncate text-foreground">{table}</span>}
            {table && (
              <Badge variant="outline" className={cn('text-[10px] ml-1 border font-mono', OP_COLORS[operation])}>
                {operation.toUpperCase()}
              </Badge>
            )}
          </div>

          {execMeta && (
            <div className="ml-auto flex items-center gap-3 shrink-0">
              <span className="text-xs text-emerald-400">{execMeta.count} row{execMeta.count !== 1 ? 's' : ''}</span>
              <span className="text-xs text-muted-foreground">{execMeta.ms} ms</span>
            </div>
          )}

          {rows.length > 0 && (
            <Tabs value={resultView} onValueChange={(v: string) => setResultView(v as 'table' | 'json')} className="ml-auto">
              <TabsList className="h-7">
                <TabsTrigger value="table" className="text-xs h-6 px-2.5 gap-1.5">
                  <Table2 className="w-3 h-3" /> Table
                </TabsTrigger>
                <TabsTrigger value="json" className="text-xs h-6 px-2.5 gap-1.5">
                  <Braces className="w-3 h-3" /> JSON
                </TabsTrigger>
              </TabsList>
            </Tabs>
          )}
        </div>

        {/* Result body */}
        <div className="flex-1 overflow-hidden flex flex-col">
          {execError && (
            <div className="m-5 rounded-lg border border-destructive/30 bg-destructive/10 px-4 py-3">
              <div className="flex items-start gap-2.5">
                <AlertCircle className="w-4 h-4 text-destructive mt-0.5 shrink-0" />
                <div>
                  <p className="text-sm font-medium text-destructive mb-0.5">Query Error</p>
                  <p className="text-xs text-destructive/80 font-mono break-all">{execError}</p>
                </div>
              </div>
            </div>
          )}

          {!execError && !execMeta && !executeMutation.isPending && (
            <div className="flex-1 flex flex-col items-center justify-center text-center p-8">
              <div className="w-12 h-12 rounded-xl bg-white/5 flex items-center justify-center mb-4">
                <Play className="w-5 h-5 text-muted-foreground" />
              </div>
              <p className="text-sm font-medium mb-1">Run a query to see results</p>
              <p className="text-xs text-muted-foreground max-w-xs">
                Select a database and table on the left, configure your query, then click <strong>Run Query</strong>.
              </p>
              {!database && (
                <ul className="mt-4 space-y-1 text-xs text-muted-foreground/60">
                  <li className="flex items-center gap-2"><ChevronRight className="w-3 h-3" /> Pick a database</li>
                  <li className="flex items-center gap-2"><ChevronRight className="w-3 h-3" /> Select a table</li>
                  <li className="flex items-center gap-2"><ChevronRight className="w-3 h-3" /> Choose an operation</li>
                </ul>
              )}
            </div>
          )}

          {executeMutation.isPending && (
            <div className="flex-1 flex items-center justify-center">
              <RefreshCw className="w-5 h-5 text-muted-foreground animate-spin" />
            </div>
          )}

          {!execError && execMeta && !executeMutation.isPending && (
            resultView === 'table'
              ? <ResultTable rows={rows} />
              : (
                <div className="flex-1 overflow-auto p-4">
                  <pre className="text-xs font-mono text-muted-foreground whitespace-pre-wrap break-all">
                    {JSON.stringify(rows, null, 2)}
                  </pre>
                </div>
              )
          )}
        </div>
      </main>
    </div>
  )
}
