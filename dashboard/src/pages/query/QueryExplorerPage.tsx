import { useState, useCallback } from 'react'
import { useQuery, useMutation } from '@tanstack/react-query'
import {
  useReactTable,
  getCoreRowModel,
  flexRender,
  type ColumnDef,
} from '@tanstack/react-table'
import { apiFetch, gatewayFetch } from '@/lib/api'
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
  ChevronRight, AlertCircle, Download, Code2, Check,
} from 'lucide-react'
import {
  Dialog, DialogContent, DialogHeader, DialogTitle,
} from '@/components/ui/dialog'
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

const STRATEGY_COLORS: Record<string, string> = {
  single:  'bg-white/5 text-muted-foreground border-white/10',
  batched: 'bg-purple-500/15 text-purple-400 border-purple-500/20',
}

interface QueryMeta {
  strategy: string
  complexity: number
  elapsed_ms: number
  rows: number
  sql: string
  request_id?: string
}

interface EngineDebug {
  limits: {
    default_rows: number
    max_rows: number
    max_complexity: number
    max_nest_depth: number
    timeout_ms: number
  }
  cache: { schema_entries: number; plan_entries: number }
  version: string
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

function safeParseJson(s: string): { ok: boolean; value: unknown; error?: string } {
  try {
    return { ok: true, value: JSON.parse(s) }
  } catch (e) {
    return { ok: false, value: null, error: (e as Error).message }
  }
}

function exportCsv(rows: Record<string, unknown>[], filename = 'query-results.csv') {
  if (rows.length === 0) return
  const cols = Object.keys(rows[0])
  const escape = (v: unknown) => {
    if (v === null || v === undefined) return ''
    const s = typeof v === 'object' ? JSON.stringify(v) : String(v)
    return `"${s.replace(/"/g, '""')}"`
  }
  const csv = [cols.join(','), ...rows.map((r) => cols.map((c) => escape(r[c])).join(','))].join('\n')
  const blob = new Blob([csv], { type: 'text/csv' })
  const url = URL.createObjectURL(blob)
  const a = Object.assign(document.createElement('a'), { href: url, download: filename })
  a.click()
  URL.revokeObjectURL(url)
}

function parseQueryError(msg: string): { title: string; detail: string; type: 'complexity' | 'depth' | 'timeout' | 'generic' } {
  if (/query too complex/i.test(msg)) {
    const m = msg.match(/score (\d+) exceeds limit (\d+)/)
    return {
      title: 'Query Too Complex',
      detail: m
        ? `Complexity score ${m[1]} exceeds the configured limit of ${m[2]}. Simplify filters or reduce nested selectors.`
        : msg,
      type: 'complexity',
    }
  }
  if (/nesting too deep/i.test(msg)) {
    const m = msg.match(/depth (\d+) exceeds limit (\d+)/)
    return {
      title: 'Query Too Deeply Nested',
      detail: m
        ? `Relationship depth ${m[1]} exceeds the configured limit of ${m[2]}. Flatten the selector tree or split into separate queries.`
        : msg,
      type: 'depth',
    }
  }
  if (/timed out/i.test(msg)) {
    return {
      title: 'Query Timed Out',
      detail: 'Execution exceeded the time limit. Add an index on the filtered column or reduce the result set with more specific filters.',
      type: 'timeout',
    }
  }
  return { title: 'Query Error', detail: msg, type: 'generic' }
}

// ─── Sub-components ───────────────────────────────────────────────────────────

function JsonExpandDialog({ value, onClose }: { value: unknown; onClose: () => void }) {
  return (
    <Dialog open onOpenChange={(o) => { if (!o) onClose() }}>
      <DialogContent className="max-w-2xl max-h-[70vh] overflow-auto">
        <DialogHeader>
          <DialogTitle className="text-sm font-mono">JSON Value</DialogTitle>
        </DialogHeader>
        <pre className="text-xs font-mono text-muted-foreground whitespace-pre-wrap mt-2">
          {JSON.stringify(value, null, 2)}
        </pre>
      </DialogContent>
    </Dialog>
  )
}

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

function ResultTable({ rows, onExpand }: { rows: Record<string, unknown>[]; onExpand: (v: unknown) => void }) {
  const [copiedCell, setCopiedCell] = useState<string | null>(null)

  const copyValue = (cellId: string, v: unknown) => {
    const text = typeof v === 'object' ? JSON.stringify(v, null, 2) : String(v ?? '')
    navigator.clipboard.writeText(text).catch(() => {})
    setCopiedCell(cellId)
    setTimeout(() => setCopiedCell((p) => (p === cellId ? null : p)), 1500)
  }

  const columns: ColumnDef<Record<string, unknown>>[] = rows.length === 0 ? [] :
    Object.keys(rows[0]).map((key) => ({
      id: key,
      accessorKey: key,
      header: key,
      cell: ({ getValue, row }) => {
        const v = getValue()
        const cellId = `${row.id}-${key}`
        const copied = copiedCell === cellId
        if (v === null || v === undefined) return <span className="text-muted-foreground/40 text-xs italic">null</span>
        if (typeof v === 'object') return (
          <button
            className="text-purple-400 text-xs font-mono hover:text-purple-300 cursor-pointer flex items-center gap-0.5 max-w-xs truncate"
            onClick={() => onExpand(v)}
            title="Click to expand JSON"
          >
            {'{'}&hellip;{'}'}
          </button>
        )
        return (
          <button
            className={cn('text-xs font-mono text-left max-w-xs truncate cursor-pointer hover:text-foreground transition-colors flex items-center gap-1', copied ? 'text-emerald-400' : '')}
            onClick={() => copyValue(cellId, v)}
            title={copied ? 'Copied!' : 'Click to copy'}
          >
            {copied && <Check className="w-3 h-3 shrink-0" />}
            {String(v)}
          </button>
        )
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
  const [execMeta, setExecMeta] = useState<QueryMeta | null>(null)

  const debugQ = useQuery({
    queryKey: ['engine-debug', projectId],
    queryFn: () => apiFetch<EngineDebug>('/db/debug'),
    enabled: !!projectId,
    staleTime: 60_000, // limits rarely change
  })
  const [execError, setExecError] = useState<string | null>(null)
  const [showSql, setShowSql] = useState(false)
  const [expandedJson, setExpandedJson] = useState<unknown>(null)

  /* Data fetching */
  const { data: dbData } = useQuery({
    queryKey: ['databases', projectId],
    queryFn: () => apiFetch<{ databases: string[] }>('/db/databases'),
    enabled: !!projectId,
  })

  const { data: tablesData } = useQuery({
    queryKey: ['tables', projectId, database],
    queryFn: () => apiFetch<{ database: string; tables: { name: string; columns: { name: string }[] }[] }>(`/db/tables/${database}`),
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

      const result = await gatewayFetch<{ data: unknown; meta: QueryMeta }>('/db/query', {
        method: 'POST',
        body: JSON.stringify(req),
      })

      const resultRows = Array.isArray(result.data) ? result.data as Record<string, unknown>[] : []
      return { rows: resultRows, meta: result.meta }
    },
    onSuccess: ({ rows: r, meta }) => {
      setRows(r)
      setExecMeta(meta)
      setShowSql(false)
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
          {/* Engine limits — fetched from /db/debug */}
          {debugQ.data && (
            <div className="mt-2 rounded border border-white/5 bg-white/[0.02] px-2.5 py-2 space-y-1">
              <p className="text-[10px] font-medium text-muted-foreground/60 uppercase tracking-wide">Engine Limits</p>
              <div className="grid grid-cols-2 gap-x-3 gap-y-0.5 text-[10px] text-muted-foreground font-mono">
                <span>Max rows</span>     <span className="text-foreground/70">{debugQ.data.limits.max_rows.toLocaleString()}</span>
                <span>Max complexity</span><span className="text-foreground/70">{debugQ.data.limits.max_complexity}</span>
                <span>Max depth</span>    <span className="text-foreground/70">{debugQ.data.limits.max_nest_depth}</span>
                <span>Timeout</span>      <span className="text-foreground/70">{(debugQ.data.limits.timeout_ms / 1000).toFixed(0)} s</span>
              </div>
            </div>
          )}
        </div>
      </aside>

      {/* Right panel — Results */}
      <main className="flex-1 flex flex-col overflow-hidden">
        {/* Result header */}
        <div className="flex items-center gap-2 px-5 py-2.5 border-b border-white/5 shrink-0 flex-wrap">
          {/* Breadcrumb */}
          <div className="flex items-center gap-1.5 text-muted-foreground min-w-0 mr-1">
            {database && <span className="text-xs truncate">{database}</span>}
            {database && table && <ChevronRight className="w-3 h-3 shrink-0" />}
            {table && <span className="text-xs truncate text-foreground">{table}</span>}
            {table && (
              <Badge variant="outline" className={cn('text-[10px] ml-1 border font-mono', OP_COLORS[operation])}>
                {operation.toUpperCase()}
              </Badge>
            )}
          </div>

          {execMeta && (
            <>
              <Badge variant="outline" className="text-[10px] border-emerald-500/20 bg-emerald-500/10 text-emerald-400">
                {execMeta.rows} row{execMeta.rows !== 1 ? 's' : ''}
              </Badge>
              <Badge variant="outline" className="text-[10px] border-white/10 bg-white/5 text-muted-foreground">
                {execMeta.elapsed_ms} ms
              </Badge>
              <Badge
                variant="outline"
                className={cn('text-[10px] border font-mono', STRATEGY_COLORS[execMeta.strategy] ?? STRATEGY_COLORS.single)}
                title={`Execution strategy: ${execMeta.strategy}`}
              >
                {execMeta.strategy}
              </Badge>
              <Badge
                variant="outline"
                className="text-[10px] border-white/10 bg-white/5 text-muted-foreground"
                title={`Complexity score: ${execMeta.complexity} (max ${debugQ.data?.limits.max_complexity ?? 1000})`}
              >
                ⚡ {execMeta.complexity}
              </Badge>
              {execMeta.request_id && (
                <Badge
                  variant="outline"
                  className="text-[10px] font-mono border-white/10 bg-white/5 text-muted-foreground cursor-pointer hover:bg-white/10"
                  title={`Request ID: ${execMeta.request_id} — click to copy`}
                  onClick={() => navigator.clipboard.writeText(execMeta.request_id!)}
                >
                  #{execMeta.request_id.slice(0, 8)}
                </Badge>
              )}
            </>
          )}

          <div className="ml-auto flex items-center gap-1.5 shrink-0">
            {execMeta?.sql && (
              <Button
                variant="ghost"
                size="sm"
                className={cn('h-7 px-2.5 text-xs gap-1.5', showSql ? 'text-sky-400 bg-sky-500/10' : 'text-muted-foreground')}
                onClick={() => setShowSql((s) => !s)}
              >
                <Code2 className="w-3.5 h-3.5" /> SQL
              </Button>
            )}
            {rows.length > 0 && (
              <Button
                variant="ghost"
                size="sm"
                className="h-7 px-2.5 text-xs gap-1.5 text-muted-foreground hover:text-foreground"
                onClick={() => exportCsv(rows, `${table}-query.csv`)}
              >
                <Download className="w-3.5 h-3.5" /> CSV
              </Button>
            )}
            {rows.length > 0 && (
              <Tabs value={resultView} onValueChange={(v: string) => setResultView(v as 'table' | 'json')}>
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
        </div>

        {/* View SQL panel */}
        {showSql && execMeta?.sql && (
          <div className="px-5 py-3 border-b border-white/5 bg-black/20 shrink-0 max-h-52 overflow-auto">
            <p className="text-[10px] text-muted-foreground/50 uppercase tracking-widest mb-1.5 font-semibold">Compiled SQL</p>
            <pre className="text-xs font-mono text-sky-300 whitespace-pre-wrap break-all">{execMeta.sql}</pre>
          </div>
        )}

        {/* Result body */}
        <div className="flex-1 overflow-hidden flex flex-col">
          {execError && (() => {
            const { title, detail, type } = parseQueryError(execError)
            const isComplexity = type === 'complexity'
            const isDepth = type === 'depth'
            const isTimeout = type === 'timeout'
            return (
              <div className={cn(
                'm-5 rounded-lg border px-4 py-3',
                isComplexity ? 'border-amber-500/30 bg-amber-500/10' :
                isDepth      ? 'border-violet-500/30 bg-violet-500/10' :
                isTimeout    ? 'border-orange-500/30 bg-orange-500/10' :
                'border-destructive/30 bg-destructive/10'
              )}>
                <div className="flex items-start gap-2.5">
                  <AlertCircle className={cn('w-4 h-4 mt-0.5 shrink-0',
                    isComplexity ? 'text-amber-400' : isDepth ? 'text-violet-400' : isTimeout ? 'text-orange-400' : 'text-destructive'
                  )} />
                  <div>
                    <p className={cn('text-sm font-medium mb-0.5',
                      isComplexity ? 'text-amber-400' : isDepth ? 'text-violet-400' : isTimeout ? 'text-orange-400' : 'text-destructive'
                    )}>{title}</p>
                    <p className={cn('text-xs font-mono break-all',
                      isComplexity ? 'text-amber-400/70' : isDepth ? 'text-violet-400/70' : isTimeout ? 'text-orange-400/70' : 'text-destructive/80'
                    )}>{detail}</p>
                  </div>
                </div>
              </div>
            )
          })()}

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
              ? <ResultTable rows={rows} onExpand={setExpandedJson} />
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

      {expandedJson !== null && (
        <JsonExpandDialog value={expandedJson} onClose={() => setExpandedJson(null)} />
      )}
    </div>
  )
}
