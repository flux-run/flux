import { useMemo, useState } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { useParams } from 'react-router-dom'
import {
  ReactFlow,
  Background,
  Controls,
  MiniMap,
  type Node,
  type Edge,
  MarkerType,
} from '@xyflow/react'
import '@xyflow/react/dist/style.css'
import { Plus, Trash2, AlertCircle } from 'lucide-react'
import { apiFetch } from '@/lib/api'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import {
  Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription, DialogFooter,
} from '@/components/ui/dialog'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'

interface Relationship {
  id: string
  schema: string
  from_table: string
  from_column: string
  to_table: string
  to_column: string
  relationship: string
  alias: string
}

interface RelResponse { relationships: Relationship[] }

interface Props { database: string; table: string }

function tableNode(id: string, label: string, x: number, y: number, highlight = false): Node {
  return {
    id,
    position: { x, y },
    data: { label },
    style: {
      background: highlight ? 'hsl(var(--primary)/0.12)' : 'hsl(var(--card))',
      border: `1.5px solid ${highlight ? 'hsl(var(--primary)/0.6)' : 'hsl(var(--border))'}`,
      borderRadius: 10,
      padding: '10px 18px',
      fontSize: 13,
      fontWeight: highlight ? 600 : 400,
      color: 'hsl(var(--foreground))',
      minWidth: 120,
    },
  }
}

export default function TableRelationshipsView({ database, table }: Props) {
  const { projectId } = useParams<{ projectId: string }>()
  const queryClient = useQueryClient()
  const [createOpen, setCreateOpen] = useState(false)
  const [form, setForm] = useState({
    from_column: '',
    to_table: '',
    to_column: '',
    relationship: 'many-to-one',
    alias: '',
  })

  const { data, isLoading, error } = useQuery({
    queryKey: ['relationships', projectId, database, table],
    queryFn: () => apiFetch<RelResponse>('/db/relationships'),
    enabled: !!projectId,
  })

  const deleteMutation = useMutation({
    mutationFn: (id: string) => apiFetch(`/db/relationships/${id}`, { method: 'DELETE' }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['relationships'] }),
  })

  const createMutation = useMutation({
    mutationFn: () =>
      apiFetch('/db/relationships', {
        method: 'POST',
        body: JSON.stringify({ database, from_table: table, ...form }),
      }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['relationships'] })
      setCreateOpen(false)
    },
  })

  // All relationships touching this table
  const rels = useMemo(
    () =>
      (data?.relationships ?? []).filter(
        (r) => r.from_table === table || r.to_table === table,
      ),
    [data, table],
  )

  // React Flow nodes + edges
  const { nodes, edges } = useMemo(() => {
    const tableSet = new Set<string>([table])
    rels.forEach((r) => {
      tableSet.add(r.from_table)
      tableSet.add(r.to_table)
    })

    const tableList = [...tableSet]
    const nodes: Node[] = tableList.map((t, i) => {
      const angle = tableList.length === 1 ? 0 : (i / tableList.length) * 2 * Math.PI
      const isCenter = t === table
      const x = isCenter ? 300 : 300 + Math.cos(angle) * 250
      const y = isCenter ? 200 : 200 + Math.sin(angle) * 180
      return tableNode(t, t, x, y, isCenter)
    })

    const edges: Edge[] = rels.map((r) => ({
      id: r.id,
      source: r.from_table,
      target: r.to_table,
      label: r.alias || `${r.from_column} → ${r.to_column}`,
      labelStyle: { fontSize: 10, fill: 'hsl(var(--muted-foreground))' },
      labelBgStyle: { fill: 'hsl(var(--background))', fillOpacity: 0.8 },
      labelBgPadding: [4, 6] as [number, number],
      markerEnd: { type: MarkerType.ArrowClosed },
      style: { strokeWidth: 1.5, stroke: 'hsl(var(--primary)/0.5)' },
      animated: false,
    }))

    return { nodes, edges }
  }, [rels, table])

  if (error) {
    return (
      <div className="flex flex-col items-center justify-center py-20 gap-2 text-muted-foreground">
        <AlertCircle className="w-6 h-6 text-destructive" />
        <p className="text-sm">{String((error as Error).message)}</p>
      </div>
    )
  }

  return (
    <div className="flex flex-col h-full">
      {/* Toolbar */}
      <div className="flex items-center justify-between px-6 py-3 border-b bg-muted/20">
        <p className="text-xs text-muted-foreground">
          {rels.length} relationship{rels.length !== 1 ? 's' : ''} on <strong>{table}</strong>
        </p>
        <Button size="sm" className="h-7 text-xs gap-1" onClick={() => setCreateOpen(true)}>
          <Plus className="w-3.5 h-3.5" /> Add relationship
        </Button>
      </div>

      {/* Graph */}
      {isLoading ? (
        <div className="flex-1 flex items-center justify-center text-muted-foreground text-sm">
          Loading…
        </div>
      ) : nodes.length <= 1 && rels.length === 0 ? (
        <div className="flex-1 flex flex-col items-center justify-center gap-2 text-muted-foreground">
          <p className="text-sm">No relationships defined for <strong>{table}</strong></p>
          <Button size="sm" variant="outline" onClick={() => setCreateOpen(true)}>
            <Plus className="w-3.5 h-3.5 mr-1.5" /> Add first relationship
          </Button>
        </div>
      ) : (
        <div className="flex-1 relative">
          <ReactFlow
            nodes={nodes}
            edges={edges}
            fitView
            fitViewOptions={{ padding: 0.3 }}
            nodesDraggable
            nodesConnectable={false}
            attributionPosition="bottom-right"
          >
            <Background gap={20} color="hsl(var(--muted-foreground)/0.1)" />
            <Controls />
            <MiniMap
              nodeColor={() => 'hsl(var(--muted))'}
              style={{ background: 'hsl(var(--card))' }}
            />
          </ReactFlow>
        </div>
      )}

      {/* Relationship list below graph */}
      {rels.length > 0 && (
        <div className="border-t bg-card max-h-52 overflow-y-auto">
          {rels.map((r) => (
            <div key={r.id} className="group flex items-center justify-between px-5 py-3 border-b last:border-0 hover:bg-muted/20 transition-colors">
              <div>
                <span className="text-xs font-mono">
                  <strong>{r.from_table}</strong>.{r.from_column}
                  <span className="text-muted-foreground"> → </span>
                  <strong>{r.to_table}</strong>.{r.to_column}
                </span>
                {r.alias && (
                  <span className="ml-2 text-[10px] text-muted-foreground">({r.alias})</span>
                )}
              </div>
              <div className="flex items-center gap-2">
                <span className="text-[10px] text-muted-foreground">{r.relationship}</span>
                <Button
                  variant="ghost" size="icon" className="w-6 h-6 opacity-0 group-hover:opacity-100 transition-opacity"
                  onClick={() => deleteMutation.mutate(r.id)}
                >
                  <Trash2 className="w-3 h-3 text-muted-foreground" />
                </Button>
              </div>
            </div>
          ))}
        </div>
      )}

      {/* Create dialog */}
      <Dialog open={createOpen} onOpenChange={setCreateOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Add relationship</DialogTitle>
            <DialogDescription>
              Define a foreign-key relationship from <strong>{table}</strong> to another table.
            </DialogDescription>
          </DialogHeader>
          <div className="space-y-3 py-2">
            <div className="grid grid-cols-2 gap-3">
              <div className="space-y-1.5">
                <Label>From column</Label>
                <Input placeholder="e.g. user_id" value={form.from_column} onChange={(e) => setForm((f) => ({ ...f, from_column: e.target.value }))} />
              </div>
              <div className="space-y-1.5">
                <Label>To table</Label>
                <Input placeholder="e.g. users" value={form.to_table} onChange={(e) => setForm((f) => ({ ...f, to_table: e.target.value }))} />
              </div>
            </div>
            <div className="grid grid-cols-2 gap-3">
              <div className="space-y-1.5">
                <Label>To column</Label>
                <Input placeholder="e.g. id" value={form.to_column} onChange={(e) => setForm((f) => ({ ...f, to_column: e.target.value }))} />
              </div>
              <div className="space-y-1.5">
                <Label>Alias (selector name)</Label>
                <Input placeholder="e.g. author" value={form.alias} onChange={(e) => setForm((f) => ({ ...f, alias: e.target.value }))} />
              </div>
            </div>
            <div className="space-y-1.5">
              <Label>Cardinality</Label>
              <Select value={form.relationship} onValueChange={(v) => setForm((f) => ({ ...f, relationship: v }))}>
                <SelectTrigger><SelectValue /></SelectTrigger>
                <SelectContent>
                  <SelectItem value="many-to-one">many-to-one (FK → PK)</SelectItem>
                  <SelectItem value="one-to-many">one-to-many (PK ← FK)</SelectItem>
                  <SelectItem value="many-to-many">many-to-many</SelectItem>
                </SelectContent>
              </Select>
            </div>
          </div>
          <DialogFooter>
            <Button variant="ghost" onClick={() => setCreateOpen(false)}>Cancel</Button>
            <Button
              onClick={() => createMutation.mutate()}
              disabled={!form.from_column || !form.to_table || !form.to_column || createMutation.isPending}
            >
              {createMutation.isPending ? 'Saving…' : 'Add relationship'}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  )
}
