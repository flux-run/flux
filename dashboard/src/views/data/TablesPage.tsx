'use client'

import { useState } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { useParams, useRouter } from 'next/navigation'
import Link from 'next/link'
import { Database, Table2, Plus, Trash2, ChevronRight, ChevronLeft, FileText, Cpu } from 'lucide-react'
import { apiFetch } from '@/lib/api'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import {
  Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription, DialogFooter,
} from '@/components/ui/dialog'
import {
  DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'

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

const COLUMN_TYPES = [
  'uuid', 'text', 'varchar', 'integer', 'bigint', 'boolean',
  'timestamptz', 'date', 'jsonb', 'float8', 'numeric',
]

export default function TablesPage() {
  const { database } = useParams() as any
  const router = useRouter()
  const queryClient = useQueryClient()
  const [createOpen, setCreateOpen] = useState(false)
  const [tableName, setTableName] = useState('')
  const [cols, setCols] = useState([
    { name: 'id', type: 'uuid', fb_type: 'default', primary_key: true, not_null: true },
    { name: 'created_at', type: 'timestamptz', fb_type: 'default', primary_key: false, not_null: true },
  ])

  const { data, isLoading } = useQuery({
    queryKey: ['tables', database],
    queryFn: () => apiFetch<TablesResponse>(`/db/tables/${database}`),
    enabled: !!database,
  })

  const createMutation = useMutation({
    mutationFn: () =>
      apiFetch('/db/tables', {
        method: 'POST',
        body: JSON.stringify({ database, name: tableName, columns: cols }),
      }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['tables'] })
      setTableName('')
      setCreateOpen(false)
    },
  })

  const dropMutation = useMutation({
    mutationFn: (table: string) => apiFetch(`/db/tables/${database}/${table}`, { method: 'DELETE' }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['tables'] }),
  })

  const tables = data?.tables ?? []

  const addCol = () =>
    setCols((c) => [...c, { name: '', type: 'text', fb_type: 'default', primary_key: false, not_null: false }])

  const updateCol = (i: number, field: string, value: string | boolean) =>
    setCols((c) => c.map((col, idx) => (idx === i ? { ...col, [field]: value } : col)))

  return (
    <div className="p-8 max-w-5xl mx-auto">
      {/* Breadcrumb */}
      <div className="flex items-center gap-1.5 text-xs text-muted-foreground mb-6">
        <Link href={`/dashboard/data`} className="flex items-center gap-1 hover:text-foreground transition-colors">
          <Database className="w-3 h-3" /> Data
        </Link>
        <ChevronRight className="w-3 h-3" />
        <span className="text-foreground font-medium">{database}</span>
      </div>

      <div className="flex items-center justify-between mb-8">
        <div>
          <div className="flex items-center gap-2 mb-1">
            <Button variant="ghost" size="icon" className="w-7 h-7" onClick={() => router.push(`/dashboard/data`)}>
              <ChevronLeft className="w-4 h-4" />
            </Button>
            <h1 className="text-2xl font-bold">{database}</h1>
          </div>
          <p className="text-sm text-muted-foreground pl-9">
            {tables.length} {tables.length === 1 ? 'table' : 'tables'}
          </p>
        </div>
        <Button size="sm" onClick={() => setCreateOpen(true)}>
          <Plus className="w-4 h-4 mr-1.5" /> New table
        </Button>
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
          <p className="text-xs text-muted-foreground mt-1 mb-4">Add your first table to this database</p>
          <Button size="sm" variant="outline" onClick={() => setCreateOpen(true)}>
            <Plus className="w-4 h-4 mr-1.5" /> Create table
          </Button>
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
                  <DropdownMenu>
                    <DropdownMenuTrigger asChild onClick={(e) => e.stopPropagation()}>
                      <Button variant="ghost" size="icon" className="w-7 h-7 opacity-0 group-hover:opacity-100">
                        <Trash2 className="w-3.5 h-3.5 text-muted-foreground" />
                      </Button>
                    </DropdownMenuTrigger>
                    <DropdownMenuContent align="end" onClick={(e) => e.stopPropagation()}>
                      <DropdownMenuItem
                        className="text-destructive focus:text-destructive"
                        onClick={() => dropMutation.mutate(t.name)}
                      >
                        Drop table
                      </DropdownMenuItem>
                    </DropdownMenuContent>
                  </DropdownMenu>
                  <ChevronRight className="w-4 h-4 text-muted-foreground opacity-0 group-hover:opacity-100 transition-opacity" />
                </div>
              </div>
            )
          })}
        </div>
      )}

      {/* Create table dialog */}
      <Dialog open={createOpen} onOpenChange={setCreateOpen}>
        <DialogContent className="max-w-2xl">
          <DialogHeader>
            <DialogTitle>New table in {database}</DialogTitle>
            <DialogDescription>Define columns for the new table.</DialogDescription>
          </DialogHeader>
          <div className="space-y-4 py-2">
            <div className="space-y-1.5">
              <Label>Table name</Label>
              <Input
                placeholder="e.g. users"
                value={tableName}
                onChange={(e) => setTableName(e.target.value)}
              />
            </div>
            <div className="space-y-2">
              <div className="flex items-center justify-between">
                <Label>Columns</Label>
                <Button variant="ghost" size="sm" className="h-7 text-xs" onClick={addCol}>
                  <Plus className="w-3 h-3 mr-1" /> Add column
                </Button>
              </div>
              <div className="rounded-lg border overflow-hidden">
                <div className="grid grid-cols-[1fr_140px_100px_60px] gap-2 px-3 py-2 bg-muted/30 border-b">
                  {['Name', 'Type', 'Kind', ''].map((h) => (
                    <p key={h} className="text-[10px] font-semibold uppercase tracking-wide text-muted-foreground">{h}</p>
                  ))}
                </div>
                {cols.map((col, i) => (
                  <div key={i} className="grid grid-cols-[1fr_140px_100px_60px] gap-2 px-3 py-2 border-b last:border-0 items-center">
                    <Input
                      value={col.name}
                      onChange={(e) => updateCol(i, 'name', e.target.value)}
                      placeholder="column_name"
                      className="h-7 text-xs"
                    />
                    <Select value={col.type} onValueChange={(v) => updateCol(i, 'type', v)}>
                      <SelectTrigger className="h-7 text-xs"><SelectValue /></SelectTrigger>
                      <SelectContent>
                        {COLUMN_TYPES.map((t) => (
                          <SelectItem key={t} value={t}>{t}</SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                    <Select value={col.fb_type} onValueChange={(v) => updateCol(i, 'fb_type', v)}>
                      <SelectTrigger className="h-7 text-xs"><SelectValue /></SelectTrigger>
                      <SelectContent>
                        <SelectItem value="default">primitive</SelectItem>
                        <SelectItem value="file">file</SelectItem>
                        <SelectItem value="computed">computed</SelectItem>
                      </SelectContent>
                    </Select>
                    <Button
                      variant="ghost" size="icon" className="h-7 w-7"
                      onClick={() => setCols((c) => c.filter((_, idx) => idx !== i))}
                    >
                      <Trash2 className="w-3 h-3 text-muted-foreground" />
                    </Button>
                  </div>
                ))}
              </div>
            </div>
          </div>
          <DialogFooter>
            <Button variant="ghost" onClick={() => setCreateOpen(false)}>Cancel</Button>
            <Button
              onClick={() => createMutation.mutate()}
              disabled={!tableName || createMutation.isPending}
            >
              {createMutation.isPending ? 'Creating…' : 'Create table'}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  )
}
