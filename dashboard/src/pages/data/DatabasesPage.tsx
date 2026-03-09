import { useState } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { useNavigate, useParams } from 'react-router-dom'
import { Database, Plus, Trash2, ChevronRight, Table2 } from 'lucide-react'
import { dbFetch } from '@/lib/api'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Badge } from '@/components/ui/badge'
import {
  Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription, DialogFooter,
} from '@/components/ui/dialog'
import {
  DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'

interface TableInfo { name: string; columns: unknown[] }
interface DbList { databases: string[] }
interface TablesResponse { database: string; tables: TableInfo[] }

export default function DatabasesPage() {
  const { projectId } = useParams<{ projectId: string }>()
  const navigate = useNavigate()
  const queryClient = useQueryClient()
  const [createOpen, setCreateOpen] = useState(false)
  const [dbName, setDbName] = useState('')

  // --- list databases
  const { data, isLoading } = useQuery({
    queryKey: ['databases', projectId],
    queryFn: () => dbFetch<DbList>('/db/databases'),
    enabled: !!projectId,
  })

  // --- table counts per db (secondary fetch)
  const dbNames = data?.databases ?? []
  const tableCounts = useQuery({
    queryKey: ['db-table-counts', projectId, dbNames.join(',')],
    queryFn: async () => {
      const counts: Record<string, number> = {}
      await Promise.all(
        dbNames.map(async (db) => {
          try {
            const r = await dbFetch<TablesResponse>(`/db/tables/${db}`)
            counts[db] = r.tables?.length ?? 0
          } catch {
            counts[db] = 0
          }
        }),
      )
      return counts
    },
    enabled: dbNames.length > 0,
  })

  const createMutation = useMutation({
    mutationFn: () => dbFetch('/db/databases', { method: 'POST', body: JSON.stringify({ name: dbName }) }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['databases'] })
      setDbName('')
      setCreateOpen(false)
    },
  })

  const dropMutation = useMutation({
    mutationFn: (name: string) => dbFetch(`/db/databases/${name}`, { method: 'DELETE' }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['databases'] }),
  })

  return (
    <div className="p-8 max-w-5xl mx-auto">
      <div className="flex items-center justify-between mb-8">
        <div>
          <h1 className="text-2xl font-bold">Data</h1>
          <p className="text-sm text-muted-foreground mt-0.5">
            Databases, tables, schemas, relationships and policies
          </p>
        </div>
        <Button size="sm" onClick={() => setCreateOpen(true)}>
          <Plus className="w-4 h-4 mr-1.5" /> New database
        </Button>
      </div>

      {isLoading ? (
        <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4">
          {[...Array(3)].map((_, i) => (
            <div key={i} className="h-28 rounded-xl border bg-card animate-pulse" />
          ))}
        </div>
      ) : dbNames.length === 0 ? (
        <div className="flex flex-col items-center justify-center py-20 text-center border rounded-xl bg-card">
          <Database className="w-10 h-10 text-muted-foreground/40 mb-3" />
          <p className="font-medium text-sm">No databases yet</p>
          <p className="text-xs text-muted-foreground mt-1 mb-4">
            Create your first database to start storing data
          </p>
          <Button size="sm" variant="outline" onClick={() => setCreateOpen(true)}>
            <Plus className="w-4 h-4 mr-1.5" /> Create database
          </Button>
        </div>
      ) : (
        <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4">
          {dbNames.map((db) => {
            const count = tableCounts.data?.[db] ?? '…'
            return (
              <div
                key={db}
                className="group relative flex flex-col gap-3 p-5 rounded-xl border bg-card hover:border-primary/40 hover:shadow-sm transition-all cursor-pointer"
                onClick={() => navigate(`/dashboard/projects/${projectId}/data/${db}`)}
              >
                <div className="flex items-start justify-between">
                  <div className="flex items-center gap-2.5">
                    <div className="flex items-center justify-center w-9 h-9 rounded-lg bg-primary/10">
                      <Database className="w-4 h-4 text-primary" />
                    </div>
                    <div>
                      <p className="font-semibold text-sm">{db}</p>
                      <p className="text-xs text-muted-foreground mt-0.5 flex items-center gap-1">
                        <Table2 className="w-3 h-3" />
                        {count} {count === 1 ? 'table' : 'tables'}
                      </p>
                    </div>
                  </div>
                  <div className="flex items-center gap-1 opacity-0 group-hover:opacity-100 transition-opacity">
                    <DropdownMenu>
                      <DropdownMenuTrigger asChild onClick={(e) => e.stopPropagation()}>
                        <Button variant="ghost" size="icon" className="w-7 h-7">
                          <Trash2 className="w-3.5 h-3.5 text-muted-foreground" />
                        </Button>
                      </DropdownMenuTrigger>
                      <DropdownMenuContent align="end" onClick={(e) => e.stopPropagation()}>
                        <DropdownMenuItem
                          className="text-destructive focus:text-destructive"
                          onClick={() => dropMutation.mutate(db)}
                        >
                          Drop database
                        </DropdownMenuItem>
                      </DropdownMenuContent>
                    </DropdownMenu>
                    <ChevronRight className="w-4 h-4 text-muted-foreground" />
                  </div>
                </div>
                <Badge variant="secondary" className="w-fit text-[10px]">
                  {db}
                </Badge>
              </div>
            )
          })}
        </div>
      )}

      {/* Create database dialog */}
      <Dialog open={createOpen} onOpenChange={setCreateOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>New database</DialogTitle>
            <DialogDescription>
              Creates a new isolated schema namespace for your project.
            </DialogDescription>
          </DialogHeader>
          <div className="space-y-3 py-2">
            <div className="space-y-1.5">
              <Label>Database name</Label>
              <Input
                placeholder="e.g. main"
                value={dbName}
                onChange={(e) => setDbName(e.target.value)}
                onKeyDown={(e) => e.key === 'Enter' && dbName && createMutation.mutate()}
              />
            </div>
          </div>
          <DialogFooter>
            <Button variant="ghost" onClick={() => setCreateOpen(false)}>Cancel</Button>
            <Button
              onClick={() => createMutation.mutate()}
              disabled={!dbName || createMutation.isPending}
            >
              {createMutation.isPending ? 'Creating…' : 'Create'}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  )
}
