import { useState } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { useParams } from 'react-router-dom'
import { Upload, Layers, CheckCircle2, Circle, ArrowLeft } from 'lucide-react'
import { Link } from 'react-router-dom'
import { apiFetch } from '@/lib/api'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import {
  Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription, DialogFooter,
} from '@/components/ui/dialog'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'

interface Fn { id: string; name: string; runtime: string }
interface Deployment { id: string; version: number; is_active: boolean; created_at: string }

export default function FunctionDetailPage() {
  const { projectId, functionId } = useParams<{ projectId: string; functionId: string }>()
  const queryClient = useQueryClient()
  const [uploadOpen, setUploadOpen] = useState(false)
  const [storageKey, setStorageKey] = useState('')

  const fnQuery = useQuery({
    queryKey: ['function', functionId],
    queryFn: () => apiFetch<Fn>(`/functions/${functionId}`),
    enabled: !!functionId,
  })

  const depQuery = useQuery({
    queryKey: ['deployments', functionId],
    queryFn: () => apiFetch<{ deployments: Deployment[] }>(`/functions/${functionId}/deployments`),
    enabled: !!functionId,
  })

  const uploadMutation = useMutation({
    mutationFn: () =>
      apiFetch(`/functions/${functionId}/deployments`, {
        method: 'POST',
        body: JSON.stringify({ storage_key: storageKey }),
      }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['deployments', functionId] })
      setStorageKey('')
      setUploadOpen(false)
    },
  })

  const deployments = depQuery.data?.deployments ?? []
  const fn = fnQuery.data

  return (
    <div className="p-8 max-w-4xl mx-auto">
      {/* Breadcrumb */}
      <Link
        to={`/dashboard/projects/${projectId}/functions`}
        className="inline-flex items-center gap-1.5 text-sm text-muted-foreground hover:text-foreground mb-6 transition-colors"
      >
        <ArrowLeft className="w-3.5 h-3.5" /> Back to functions
      </Link>

      <div className="flex items-center justify-between mb-8">
        <div>
          <div className="flex items-center gap-3 mb-1">
            <h1 className="text-2xl font-bold">{fn?.name ?? '…'}</h1>
            {fn && (
              <Badge variant="secondary" className="font-mono text-xs">{fn.runtime}</Badge>
            )}
          </div>
          <p className="text-xs text-muted-foreground font-mono">{functionId}</p>
        </div>
        <Button onClick={() => setUploadOpen(true)}>
          <Upload className="w-4 h-4" />
          Upload deployment
        </Button>
      </div>

      {/* Deployment timeline */}
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2 text-base">
            <Layers className="w-4 h-4" />
            Deployments
          </CardTitle>
        </CardHeader>
        <CardContent>
          {depQuery.isLoading && (
            <div className="space-y-3">
              {[...Array(2)].map((_, i) => (
                <div key={i} className="h-12 rounded-lg bg-muted/40 animate-pulse" />
              ))}
            </div>
          )}

          {!depQuery.isLoading && deployments.length === 0 && (
            <div className="text-center py-8">
              <Layers className="w-7 h-7 mx-auto mb-3 text-muted-foreground/30" />
              <p className="text-sm text-muted-foreground">No deployments yet. Upload your first bundle.</p>
            </div>
          )}

          {deployments.length > 0 && (
            <div className="space-y-2">
              {deployments.map((d) => (
                <div
                  key={d.id}
                  className={`flex items-center gap-3 p-3 rounded-lg border transition-colors ${
                    d.is_active
                      ? 'border-primary/30 bg-primary/5'
                      : 'border-border hover:border-border/80'
                  }`}
                >
                  {d.is_active ? (
                    <CheckCircle2 className="w-4 h-4 text-primary shrink-0" />
                  ) : (
                    <Circle className="w-4 h-4 text-muted-foreground/40 shrink-0" />
                  )}
                  <div className="flex-1">
                    <div className="flex items-center gap-2">
                      <span className="font-mono text-sm font-medium">v{d.version}</span>
                      {d.is_active && (
                        <Badge variant="default" className="text-xs">active</Badge>
                      )}
                    </div>
                    <p className="text-xs text-muted-foreground">
                      {new Date(d.created_at).toLocaleString()}
                    </p>
                  </div>
                  <p className="text-xs text-muted-foreground font-mono truncate max-w-[120px]">
                    {d.id.slice(0, 12)}…
                  </p>
                </div>
              ))}
            </div>
          )}
        </CardContent>
      </Card>

      {/* Upload Dialog */}
      <Dialog open={uploadOpen} onOpenChange={setUploadOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Upload deployment</DialogTitle>
            <DialogDescription>
              Provide the storage key for the function bundle (e.g. an S3 object key or GCS path).
            </DialogDescription>
          </DialogHeader>
          <div className="space-y-3">
            <Label>Storage key</Label>
            <Input
              placeholder="functions/send-email/v3.tar.gz"
              value={storageKey}
              onChange={(e) => setStorageKey(e.target.value)}
            />
            {uploadMutation.isError && (
              <p className="text-sm text-destructive">{uploadMutation.error.message}</p>
            )}
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setUploadOpen(false)}>Cancel</Button>
            <Button
              onClick={() => uploadMutation.mutate()}
              disabled={!storageKey.trim() || uploadMutation.isPending}
            >
              {uploadMutation.isPending ? 'Uploading…' : 'Upload'}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  )
}
