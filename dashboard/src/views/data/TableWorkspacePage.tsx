'use client'

import { useState } from 'react'
import { useParams } from 'next/navigation'
import Link from 'next/link'
import { Database, Table2, ChevronRight } from 'lucide-react'
import { cn } from '@/lib/utils'
import TableDataView from './TableDataView'
import TableSchemaView from './TableSchemaView'
import TableRelationshipsView from './TableRelationshipsView'
import TablePoliciesView from './TablePoliciesView'
import TableHooksView from './TableHooksView'

type Tab = 'data' | 'schema' | 'relationships' | 'policies' | 'hooks'

const TABS: { id: Tab; label: string }[] = [
  { id: 'data',          label: 'Data' },
  { id: 'schema',        label: 'Schema' },
  { id: 'relationships', label: 'Relationships' },
  { id: 'policies',      label: 'Policies' },
  { id: 'hooks',         label: 'Hooks' },
]

export default function TableWorkspacePage() {
  const { projectId, database, table } = useParams() as any
  const [activeTab, setActiveTab] = useState<Tab>('data')

  return (
    <div className="flex flex-col h-full min-h-0">
      {/* Top nav */}
      <div className="flex-shrink-0 px-8 pt-6 pb-0 border-b bg-background">
        {/* Breadcrumb */}
        <div className="flex items-center gap-1.5 text-xs text-muted-foreground mb-4">
          <Link
            href={`/dashboard/projects/${projectId}/data`}
            className="flex items-center gap-1 hover:text-foreground transition-colors"
          >
            <Database className="w-3 h-3" /> Data
          </Link>
          <ChevronRight className="w-3 h-3" />
          <Link
            href={`/dashboard/projects/${projectId}/data/${database}`}
            className="hover:text-foreground transition-colors"
          >
            {database}
          </Link>
          <ChevronRight className="w-3 h-3" />
          <span className="text-foreground font-medium flex items-center gap-1">
            <Table2 className="w-3 h-3" /> {table}
          </span>
        </div>

        {/* Table name */}
        <h1 className="text-xl font-bold mb-4">{table}</h1>

        {/* Tab bar */}
        <div className="flex items-center gap-0.5 -mb-px">
          {TABS.map((tab) => (
            <button
              key={tab.id}
              onClick={() => setActiveTab(tab.id)}
              className={cn(
                'px-4 py-2 text-sm font-medium transition-colors border-b-2 -mb-px',
                activeTab === tab.id
                  ? 'border-primary text-primary'
                  : 'border-transparent text-muted-foreground hover:text-foreground hover:border-muted-foreground/30',
              )}
            >
              {tab.label}
            </button>
          ))}
        </div>
      </div>

      {/* Tab content */}
      <div className="flex-1 overflow-auto">
        {activeTab === 'data'          && <TableDataView          database={database!} table={table!} />}
        {activeTab === 'schema'        && <TableSchemaView        database={database!} table={table!} />}
        {activeTab === 'relationships' && <TableRelationshipsView database={database!} table={table!} />}
        {activeTab === 'policies'      && <TablePoliciesView      database={database!} table={table!} />}
        {activeTab === 'hooks'         && <TableHooksView         table={table!} />}
      </div>
    </div>
  )
}
