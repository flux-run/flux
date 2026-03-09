import { useMemo, useState } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { useParams } from 'react-router-dom'
import { ShieldCheck, Plus, Trash2, AlertCircle } from 'lucide-react'
import { apiFetch } from '@/lib/api'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Badge } from '@/components/ui/badge'
import {
  Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription, DialogFooter,
} from '@/components/ui/dialog'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'

interface Policy {
  id: string
  table: string
  role: string
  operation: string
  allowed_columns: string[]
  row_condition_sql: string | null
}

interface PolicyResponse { policies: Policy[] }

interface Props { database: string; table: string }

const OP_COLORS: Record<string, string> = {
  select: 'bg-sky-500/10 text-sky-700 dark:text-sky-400',
  insert: 'bg-emerald-500/10 text-emerald-700 dark:text-emerald-400',
  update: 'bg-amber-500/10 text-amber-700 dark:text-amber-400',
  delete: 'bg-red-500/10 text-red-700 dark:text-red-400',
}

export default function TablePoliciesView({ database, table }: Props) {
  const { projectId } = useParams<{ projectId: string }>()
  const queryClient = useQueryClient()
  const [createOpen, setCreateOpen] = useState(false)
  const [form, setForm] = useState({
    role: 'authenticated',
    operation: 'select',
    row_condition: '',
    allowed_columns: '',
  })

  const { data, isLoading, error } = useQuery({
    queryKey: ['policies', projectId],
    queryFn: () => apiFetch<PolicyResponse>('/db/policies'),
    enabled: !!projectId,
  })

  const policies = useMemo(
    () => (data?.policies ?? []).filter((p) => p.table === table),
    [data, table],
  )

  const deleteMutation = useMutation({
    mutationFn: (id: string) => apiFetch(`/db/policies/${id}`, { method: 'DELETE' }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['policies'] }),
  })

  const createMutation = useMutation({
    mutationFn: () =>
      apiFetch('/db/policies', {
        method: 'POST',
        body: JSON.stringify({
          database,
          table_name: table,
          role: form.role,
          operation: form.operation,
          row_condition: form.row_condition || null,
          allowed_columns: form.allowed_columns
            ? form.allowed_columns.split(',').map((c) => c.trim())
            : null,
        }),
      }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['policies'] })
      setCreateOpen(false)
    },
  })

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
      <div className="flex items-center justify-between mb-6">
        <div>
          <h2 className="text-sm font-semibold text-muted-foreground uppercase tracking-widest">
            Policies — {table}
          </h2>
          <p className="text-xs text-muted-foreground mt-0.5">
            Row-level and column-level security rules
          </p>
        </div>
        <Button size="sm" onClick={() => setCreateOpen(true)}>
          <Plus className="w-3.5 h-3.5 mr-1.5" /> Add policy
        </Button>
      </div>

      {isLoading ? (
        <div className="space-y-2">
          {[...Array(3)].map((_, i) => (
            <div key={i} className="h-16 rounded-xl border bg-card animate-pulse" />
          ))}
        </div>
      ) : policies.length === 0 ? (
        <div className="flex flex-col items-center justify-center py-16 text-center border rounded-xl bg-card gap-2">
          <ShieldCheck className="w-8 h-8 text-muted-foreground/40" />
          <p className="text-sm font-medium">No policies on {table}</p>
          <p className="text-xs text-muted-foreground">
            Without policies, all authenticated users have full access.
          </p>
          <Button size="sm" variant="outline" className="mt-2" onClick={() => setCreateOpen(true)}>
            <Plus className="w-3.5 h-3.5 mr-1.5" /> Add policy
          </Button>
        </div>
      ) : (
        <div className="rounded-xl border divide-y overflow-hidden bg-card">
          {policies.map((p) => (
            <div key={p.id} className="group flex items-start justify-between px-5 py-4 hover:bg-muted/20 transition-colors">
              <div className="space-y-1.5">
                <div className="flex items-center gap-2 flex-wrap">
                  <span className={`inline-flex text-[10px] font-semibold px-2 py-0.5 rounded-full uppercase ${OP_COLORS[p.operation] ?? 'bg-muted'}`}>
                    {p.operation}
                  </span>
                  <Badge variant="outline" className="text-[10px]">{p.role}</Badge>
                </div>
                {p.row_condition_sql && (
                  <p className="text-xs font-mono text-muted-foreground">
                    <span className="text-foreground/70">WHERE</span> {p.row_condition_sql}
                  </p>
                )}
                {Array.isArray(p.allowed_columns) && p.allowed_columns.length > 0 && (
                  <div className="flex flex-wrap gap-1">
                    {p.allowed_columns.map((col) => (
                      <Badge key={col} variant="secondary" className="text-[10px]">{col}</Badge>
                    ))}
                  </div>
                )}
              </div>
              <Button
                variant="ghost" size="icon" className="w-7 h-7 mt-0.5 opacity-0 group-hover:opacity-100 transition-opacity shrink-0"
                onClick={() => deleteMutation.mutate(p.id)}
              >
                <Trash2 className="w-3.5 h-3.5 text-muted-foreground" />
              </Button>
            </div>
          ))}
        </div>
      )}

      {/* Create dialog */}
      <Dialog open={createOpen} onOpenChange={setCreateOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Add policy</DialogTitle>
            <DialogDescription>
              Restrict access to <strong>{table}</strong> for a specific role and operation.
            </DialogDescription>
          </DialogHeader>
          <div className="space-y-3 py-2">
            <div className="grid grid-cols-2 gap-3">
              <div className="space-y-1.5">
                <Label>Role</Label>
                <Input
                  placeholder="authenticated"
                  value={form.role}
                  onChange={(e) => setForm((f) => ({ ...f, role: e.target.value }))}
                />
              </div>
              <div className="space-y-1.5">
                <Label>Operation</Label>
                <Select value={form.operation} onValueChange={(v) => setForm((f) => ({ ...f, operation: v }))}>
                  <SelectTrigger><SelectValue /></SelectTrigger>
                  <SelectContent>
                    <SelectItem value="select">select</SelectItem>
                    <SelectItem value="insert">insert</SelectItem>
                    <SelectItem value="update">update</SelectItem>
                    <SelectItem value="delete">delete</SelectItem>
                  </SelectContent>
                </Select>
              </div>
            </div>
            <div className="space-y-1.5">
              <Label>Row condition (optional)</Label>
              <Input
                placeholder="e.g. uid = user_id"
                className="font-mono text-xs"
                value={form.row_condition}
                onChange={(e) => setForm((f) => ({ ...f, row_condition: e.target.value }))}
              />
            </div>
            <div className="space-y-1.5">
              <Label>Allowed columns (comma-separated, blank = all)</Label>
              <Input
                placeholder="e.g. id, name, email"
                value={form.allowed_columns}
                onChange={(e) => setForm((f) => ({ ...f, allowed_columns: e.target.value }))}
              />
            </div>
          </div>
          <DialogFooter>
            <Button variant="ghost" onClick={() => setCreateOpen(false)}>Cancel</Button>
            <Button onClick={() => createMutation.mutate()} disabled={createMutation.isPending}>
              {createMutation.isPending ? 'Saving…' : 'Add policy'}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  )
}
