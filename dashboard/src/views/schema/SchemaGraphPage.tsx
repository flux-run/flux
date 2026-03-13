'use client'

import { useMemo, useCallback } from 'react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { useParams } from 'next/navigation'
import {
  ReactFlow,
  Background,
  BackgroundVariant,
  Controls,
  MiniMap,
  type Node,
  type Edge,
  MarkerType,
  Handle,
  Position,
} from '@xyflow/react'
import '@xyflow/react/dist/style.css'
import {
  Share2, RefreshCw, Download, Copy, Check, Loader2,
  AlertCircle, Table2, Zap,
} from 'lucide-react'
import { apiFetch } from '@/lib/api'
import { useStore } from '@/state/tenantStore'
import { PageHeader } from '@/components/layout/PageHeader'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import { useState } from 'react'
import { cn } from '@/lib/utils'

// ─── Types ────────────────────────────────────────────────────────────────────

interface Column {
  name: string
  type: string
  nullable: boolean
  is_primary_key: boolean
  table_name: string
}

interface Relationship {
  id: string
  from_table: string
  from_column: string
  to_table: string
  to_column: string
  relationship: string
  alias: string
}

interface FunctionDef {
  name: string
  description: string | null
}

interface SdkSchema {
  schema_hash: string
  schema_version: number
  tables: Array<{ name: string; schema: string }>
  columns: Column[]
  relationships: Relationship[]
  policies: unknown[]
  functions: FunctionDef[]
}

// ─── Custom Node: Table ────────────────────────────────────────────────────────

function TableNode({ data }: { data: { label: string; columns: Column[] } }) {
  const pks = data.columns.filter((c) => c.is_primary_key)
  const cols = data.columns.filter((c) => !c.is_primary_key)

  return (
    <div
      className={cn(
        'rounded-xl border bg-card text-card-foreground shadow-sm min-w-[180px] max-w-[240px]',
        'border-border/60',
      )}
    >
      <Handle type="target" position={Position.Left}  style={{ opacity: 0 }} />
      <Handle type="source" position={Position.Right} style={{ opacity: 0 }} />

      {/* Header */}
      <div className="flex items-center gap-1.5 px-3 py-2 border-b border-border/60 bg-primary/8 rounded-t-xl">
        <Table2 className="w-3.5 h-3.5 text-primary shrink-0" />
        <span className="font-semibold text-xs truncate text-foreground">{data.label}</span>
      </div>

      {/* Primary keys */}
      {pks.map((col) => (
        <div
          key={col.name}
          className="flex items-center justify-between gap-2 px-3 py-1 border-b border-border/30 bg-yellow-500/5"
        >
          <span className="text-[11px] font-medium text-foreground/90 truncate">{col.name}</span>
          <span className="text-[9px] text-yellow-600 dark:text-yellow-400 font-mono bg-yellow-500/10 px-1 rounded shrink-0">
            PK · {col.type}
          </span>
        </div>
      ))}

      {/* Other columns */}
      {cols.map((col) => (
        <div
          key={col.name}
          className="flex items-center justify-between gap-2 px-3 py-0.5"
        >
          <span className="text-[11px] text-foreground/70 truncate">{col.name}</span>
          <span className="text-[9px] text-muted-foreground font-mono shrink-0">{col.type}</span>
        </div>
      ))}

      {cols.length > 0 && <div className="pb-1.5" />}
    </div>
  )
}

// ─── Custom Node: Function ─────────────────────────────────────────────────────

function FunctionNode({ data }: { data: { label: string; description: string } }) {
  return (
    <div className="rounded-xl border border-violet-500/30 bg-violet-500/5 shadow-sm min-w-[140px] max-w-[200px]">
      <Handle type="target" position={Position.Left}  style={{ opacity: 0 }} />
      <Handle type="source" position={Position.Right} style={{ opacity: 0 }} />
      <div className="flex items-center gap-1.5 px-3 py-2">
        <Zap className="w-3.5 h-3.5 text-violet-500 shrink-0" />
        <span className="font-semibold text-xs truncate text-foreground">{data.label}</span>
      </div>
      {data.description && (
        <p className="px-3 pb-2 text-[10px] text-muted-foreground leading-tight line-clamp-2">
          {data.description}
        </p>
      )}
    </div>
  )
}

const nodeTypes = { table: TableNode, function: FunctionNode }

// ─── Layout helpers ───────────────────────────────────────────────────────────

const TABLE_W = 220
const TABLE_H = 200
const COL_GAP = 80
const ROW_GAP = 60
const COLS    = 4

function gridLayout(
  schema: SdkSchema,
): { nodes: Node[]; edges: Edge[] } {
  const tableNames = schema.tables.map((t) => t.name)

  // Table nodes
  const tableNodes: Node[] = tableNames.map((name, i) => {
    const col = i % COLS
    const row = Math.floor(i / COLS)
    return {
      id: `table:${name}`,
      type: 'table',
      position: { x: col * (TABLE_W + COL_GAP), y: row * (TABLE_H + ROW_GAP) },
      data: {
        label: name,
        columns: schema.columns.filter((c) => c.table_name === name),
      },
    }
  })

  // Function nodes (row below all tables)
  const tableRows = Math.ceil(tableNames.length / COLS)
  const fnStartY  = tableRows * (TABLE_H + ROW_GAP) + 80
  const functionNodes: Node[] = schema.functions.map((fn, i) => ({
    id: `fn:${fn.name}`,
    type: 'function',
    position: { x: i * (TABLE_W / 2 + COL_GAP / 2), y: fnStartY },
    data: { label: fn.name, description: fn.description ?? '' },
  }))

  // Relationship edges
  const edges: Edge[] = schema.relationships.map((rel) => ({
    id: `rel:${rel.id}`,
    source: `table:${rel.from_table}`,
    target: `table:${rel.to_table}`,
    label: rel.alias || rel.relationship,
    labelStyle: { fontSize: 10, fill: 'hsl(var(--muted-foreground))' },
    labelBgStyle: { fill: 'hsl(var(--card))', stroke: 'none' },
    style: { strokeWidth: 1.5, stroke: 'hsl(var(--border))' },
    markerEnd: { type: MarkerType.ArrowClosed, width: 12, height: 12, color: 'hsl(var(--border))' },
    animated: rel.relationship.includes('many'),
  }))

  return { nodes: [...tableNodes, ...functionNodes], edges }
}

// ─── Main component ────────────────────────────────────────────────────────────

export default function SchemaGraphPage() {
  const { projectId } = useParams() as any
  const { projectName } = useStore()
  const queryClient = useQueryClient()
  const [copied, setCopied] = useState(false)

  const { data, isLoading, error } = useQuery({
    queryKey: ['sdk-schema', projectId],
    queryFn: () => apiFetch<SdkSchema>('/sdk/schema'),
    enabled: !!projectId,
  })

  const { nodes, edges } = useMemo(
    () => (data ? gridLayout(data) : { nodes: [], edges: [] }),
    [data],
  )

  const handleCopyHash = useCallback(async () => {
    if (!data?.schema_hash) return
    await navigator.clipboard.writeText(data.schema_hash)
    setCopied(true)
    setTimeout(() => setCopied(false), 2000)
  }, [data?.schema_hash])

  const handleDownloadSDK = useCallback(() => {
    const apiBase = (process.env.NEXT_PUBLIC_API_URL ?? 'http://localhost:8080') as string
    window.open(
      `${apiBase}/projects/${projectId}/sdk/typescript`,
      '_blank',
    )
  }, [projectId])

  const handleDownloadOpenAPI = useCallback(() => {
    const apiBase = (process.env.NEXT_PUBLIC_API_URL ?? 'http://localhost:8080') as string
    window.open(
      `${apiBase}/projects/${projectId}/openapi.json`,
      '_blank',
    )
  }, [projectId])

  return (
    <div className="flex flex-col h-full">
      <PageHeader
        title="Schema Graph"
        description={data ? `${data.tables.length} tables · ${data.relationships.length} relationships · ${data.functions.length} functions` : 'Live visual map of your tables, relationships, and functions'}
        breadcrumbs={[
          { label: 'Projects', href: '/dashboard' },
          { label: projectName ?? projectId ?? '…', href: `/dashboard/projects/${projectId}/overview` },
          { label: 'Schema' },
        ]}
        actions={
          <div className="flex items-center gap-2">
            {data && (
              <Badge variant="outline" className="text-[10px] font-mono">
                v{data.schema_version}
              </Badge>
            )}
            <Button
              variant="outline"
              size="sm"
              onClick={handleCopyHash}
              disabled={!data}
              className="gap-1.5 text-xs h-8"
            >
              {copied ? <Check className="w-3.5 h-3.5" /> : <Copy className="w-3.5 h-3.5" />}
              {copied ? 'Copied!' : 'Copy hash'}
            </Button>
            <Button
              variant="outline"
              size="sm"
              onClick={handleDownloadOpenAPI}
              disabled={!data}
              className="gap-1.5 text-xs h-8"
            >
              <Download className="w-3.5 h-3.5" />
              OpenAPI
            </Button>
            <Button
              variant="outline"
              size="sm"
              onClick={handleDownloadSDK}
              disabled={!data}
              className="gap-1.5 text-xs h-8"
            >
              <Download className="w-3.5 h-3.5" />
              SDK (.ts)
            </Button>
            <Button
              variant="outline"
              size="sm"
              onClick={() => queryClient.invalidateQueries({ queryKey: ['sdk-schema', projectId] })}
              disabled={isLoading}
              className="gap-1.5 text-xs h-8"
            >
              <RefreshCw className={cn('w-3.5 h-3.5', isLoading && 'animate-spin')} />
              Refresh
            </Button>
          </div>
        }
      />

      {/* Canvas */}
      <div className="flex-1 relative">
        {isLoading && (
          <div className="absolute inset-0 flex items-center justify-center z-10 bg-background/60">
            <Loader2 className="w-6 h-6 animate-spin text-muted-foreground" />
          </div>
        )}

        {error && (
          <div className="absolute inset-0 flex items-center justify-center z-10">
            <div className="flex flex-col items-center gap-2 text-center">
              <AlertCircle className="w-8 h-8 text-destructive" />
              <p className="text-sm font-medium">Failed to load schema</p>
              <p className="text-xs text-muted-foreground max-w-xs">
                {error instanceof Error ? error.message : 'Unknown error'}
              </p>
            </div>
          </div>
        )}

        {!isLoading && !error && data && (
          <ReactFlow
            nodes={nodes}
            edges={edges}
            nodeTypes={nodeTypes}
            fitView
            fitViewOptions={{ padding: 0.15 }}
            minZoom={0.2}
            maxZoom={2}
            proOptions={{ hideAttribution: true }}
          >
            <Background variant={BackgroundVariant.Dots} gap={20} size={1} />
            <Controls />
            <MiniMap
              nodeColor={() => 'hsl(var(--primary)/0.25)'}
              maskColor="hsl(var(--background)/0.7)"
            />
          </ReactFlow>
        )}

        {!isLoading && !error && data && nodes.length === 0 && (
          <div className="absolute inset-0 flex items-center justify-center">
            <div className="text-center text-muted-foreground">
              <Share2 className="w-10 h-10 mx-auto mb-3 opacity-20" />
              <p className="text-sm">No tables found in this project yet.</p>
              <p className="text-xs mt-1">Create tables in the Data section to see them here.</p>
            </div>
          </div>
        )}
      </div>

      {/* Footer stats */}
      {data && (
        <div className="flex items-center gap-4 px-6 py-2.5 border-t shrink-0 bg-muted/20">
          <span className="text-[11px] text-muted-foreground">
            <strong className="font-medium text-foreground">{data.tables.length}</strong> tables
          </span>
          <span className="text-[11px] text-muted-foreground">
            <strong className="font-medium text-foreground">{data.columns.length}</strong> columns
          </span>
          <span className="text-[11px] text-muted-foreground">
            <strong className="font-medium text-foreground">{data.relationships.length}</strong> relationships
          </span>
          <span className="text-[11px] text-muted-foreground">
            <strong className="font-medium text-foreground">{data.functions.length}</strong> functions
          </span>
          <span className="ml-auto text-[10px] font-mono text-muted-foreground/60 truncate max-w-[240px]">
            hash: {data.schema_hash}
          </span>
        </div>
      )}
    </div>
  )
}
