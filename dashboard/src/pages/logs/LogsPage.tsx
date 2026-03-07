import { ScrollText } from 'lucide-react'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'

export default function LogsPage() {
  return (
    <div className="p-8 max-w-5xl mx-auto">
      <div className="mb-8">
        <h1 className="text-2xl font-bold">Logs</h1>
        <p className="text-sm text-muted-foreground mt-0.5">
          Real-time function execution logs
        </p>
      </div>

      <Card>
        <CardHeader className="flex flex-row items-center gap-3">
          <div className="flex items-center justify-center w-9 h-9 rounded-lg bg-muted">
            <ScrollText className="w-4 h-4 text-muted-foreground" />
          </div>
          <div>
            <CardTitle className="text-base">Logs coming soon</CardTitle>
          </div>
        </CardHeader>
        <CardContent>
          <p className="text-sm text-muted-foreground mb-6">
            The logs system depends on the serverless execution runtime, which is the next phase of Fluxbase. Once available, this page will show:
          </p>
          {/* Preview table skeleton */}
          <div className="rounded-xl border overflow-hidden">
            <div className="grid grid-cols-[180px_1fr_100px_80px] gap-4 px-4 py-2.5 bg-muted/30 border-b">
              {['Timestamp', 'Function', 'Duration', 'Status'].map((h) => (
                <p key={h} className="text-xs font-semibold text-muted-foreground/50 uppercase tracking-wide">
                  {h}
                </p>
              ))}
            </div>
            {[...Array(4)].map((_, i) => (
              <div key={i} className="grid grid-cols-[180px_1fr_100px_80px] gap-4 px-4 py-3.5 border-b last:border-0 items-center">
                <div className="h-2.5 w-28 rounded bg-muted/40 animate-pulse" />
                <div className="h-2.5 w-24 rounded bg-muted/40 animate-pulse" />
                <div className="h-2.5 w-12 rounded bg-muted/40 animate-pulse" />
                <div className={`h-5 w-14 rounded-full animate-pulse ${i === 2 ? 'bg-red-500/20' : 'bg-emerald-500/20'}`} />
              </div>
            ))}
          </div>
        </CardContent>
      </Card>
    </div>
  )
}
