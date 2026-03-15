'use client'

import { useState } from 'react'
import { Copy, Check } from 'lucide-react'
import { PageHeader } from '@/components/layout/PageHeader'

function CopyField({ label, value }: { label: string; value: string }) {
  const [copied, setCopied] = useState(false)
  const copy = () => {
    navigator.clipboard.writeText(value)
    setCopied(true)
    setTimeout(() => setCopied(false), 1500)
  }
  return (
    <div>
      <p className="text-xs text-muted-foreground mb-1.5">{label}</p>
      <div className="flex items-center gap-2">
        <code className="flex-1 text-xs font-mono bg-muted/50 border rounded-lg px-3 py-2 text-foreground/80 truncate">
          {value}
        </code>
        <button
          onClick={copy}
          className="shrink-0 p-2 rounded-lg border hover:bg-muted/50 transition-colors text-muted-foreground hover:text-foreground"
          title="Copy"
        >
          {copied ? <Check className="w-3.5 h-3.5 text-emerald-400" /> : <Copy className="w-3.5 h-3.5" />}
        </button>
      </div>
    </div>
  )
}

export default function ProjectSettingsPage() {
  const API_BASE = process.env.NEXT_PUBLIC_API_URL ?? 'http://localhost:4000'
  const GATEWAY_BASE = process.env.NEXT_PUBLIC_GATEWAY_URL ?? 'http://localhost:4001'

  return (
    <div className="flex flex-col h-full">
      <PageHeader
        title="Settings"
        description="System information and configuration"
        breadcrumbs={[{ label: 'Settings' }]}
      />
      <div className="flex-1 overflow-y-auto">
        <div className="p-6 max-w-2xl mx-auto space-y-8">
          <section>
            <h2 className="text-sm font-semibold mb-4">Connection Info</h2>
            <div className="rounded-xl border bg-card p-5 space-y-4">
              <CopyField label="API base URL" value={API_BASE} />
              <CopyField label="Gateway base URL" value={GATEWAY_BASE} />
            </div>
          </section>
        </div>
      </div>
    </div>
  )
}
