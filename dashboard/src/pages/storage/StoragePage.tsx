import { useState } from 'react'
import { useParams } from 'react-router-dom'
import { HardDrive, FileText, Lock, Globe, Info } from 'lucide-react'
import { dbFetch } from '@/lib/api'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Badge } from '@/components/ui/badge'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'

interface UploadUrlResponse { upload_url: string; file_url: string; key: string }

export default function StoragePage() {
  useParams<{ projectId: string }>()
  const [table, setTable]   = useState('')
  const [column, setColumn] = useState('')
  const [rowId, setRowId]   = useState('')
  const [result, setResult] = useState<UploadUrlResponse | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  async function generateUploadUrl() {
    if (!table || !column || !rowId) return
    setLoading(true)
    setError(null)
    setResult(null)
    try {
      const r = await dbFetch<UploadUrlResponse>('/files/upload-url', {
        method: 'POST',
        body: JSON.stringify({ table, column, row_id: rowId }),
      })
      setResult(r)
    } catch (e) {
      setError((e as Error).message)
    } finally {
      setLoading(false)
    }
  }

  return (
    <div className="p-8 max-w-5xl mx-auto">
      <div className="mb-8">
        <h1 className="text-2xl font-bold">Storage</h1>
        <p className="text-sm text-muted-foreground mt-0.5">
          File columns use pre-signed S3 URLs for secure upload and download
        </p>
      </div>

      <div className="grid grid-cols-1 lg:grid-cols-2 gap-6">
        {/* How it works */}
        <Card>
          <CardHeader>
            <CardTitle className="flex items-center gap-2 text-base">
              <Info className="w-4 h-4 text-primary" />
              How file storage works
            </CardTitle>
          </CardHeader>
          <CardContent className="space-y-4 text-sm text-muted-foreground">
            <p>
              File columns (<Badge variant="secondary" className="text-[10px]">fb_type: file</Badge>) store
              pre-signed object keys, not raw data. Files live at:
            </p>
            <pre className="text-[11px] font-mono bg-muted/30 rounded-lg p-3 whitespace-pre-wrap">
              s3://&lt;bucket&gt;/&lt;tenant&gt;/&lt;project&gt;/&lt;table&gt;/&lt;row&gt;/&lt;column&gt;
            </pre>
            <div className="space-y-2">
              <div className="flex items-start gap-2">
                <Globe className="w-3.5 h-3.5 mt-0.5 shrink-0 text-emerald-500" />
                <p><strong className="text-foreground">Public</strong> — download URL visible without auth</p>
              </div>
              <div className="flex items-start gap-2">
                <Lock className="w-3.5 h-3.5 mt-0.5 shrink-0 text-amber-500" />
                <p><strong className="text-foreground">Private</strong> — signed URL expires after 15 minutes</p>
              </div>
            </div>
            <p>
              The dashboard and SDK request upload/download URLs from the API.
              Files are streamed directly to S3 — they never pass through the API server.
            </p>
          </CardContent>
        </Card>

        {/* Upload URL tester */}
        <Card>
          <CardHeader>
            <CardTitle className="flex items-center gap-2 text-base">
              <FileText className="w-4 h-4 text-primary" />
              Generate upload URL
            </CardTitle>
          </CardHeader>
          <CardContent className="space-y-3">
            <div className="grid grid-cols-2 gap-3">
              <div className="space-y-1.5">
                <Label>Table</Label>
                <Input
                  placeholder="e.g. users"
                  value={table}
                  onChange={(e) => setTable(e.target.value)}
                />
              </div>
              <div className="space-y-1.5">
                <Label>File column</Label>
                <Input
                  placeholder="e.g. avatar"
                  value={column}
                  onChange={(e) => setColumn(e.target.value)}
                />
              </div>
            </div>
            <div className="space-y-1.5">
              <Label>Row ID</Label>
              <Input
                placeholder="uuid of the row"
                value={rowId}
                onChange={(e) => setRowId(e.target.value)}
              />
            </div>
            <Button
              className="w-full"
              disabled={!table || !column || !rowId || loading}
              onClick={generateUploadUrl}
            >
              {loading ? 'Generating…' : 'Generate upload URL'}
            </Button>

            {error && (
              <p className="text-xs text-destructive font-medium">{error}</p>
            )}

            {result && (
              <div className="rounded-lg border bg-muted/20 p-3 space-y-2 text-xs">
                <div>
                  <p className="text-[10px] text-muted-foreground uppercase tracking-wide mb-0.5">Upload URL (PUT to this)</p>
                  <p className="font-mono break-all text-muted-foreground">{result.upload_url}</p>
                </div>
                <div>
                  <p className="text-[10px] text-muted-foreground uppercase tracking-wide mb-0.5">File key (stored in row)</p>
                  <p className="font-mono text-primary">{result.key}</p>
                </div>
              </div>
            )}
          </CardContent>
        </Card>
      </div>

      {/* File columns from schema */}
      <div className="mt-8">
        <div className="flex items-center gap-2 mb-4">
          <HardDrive className="w-4 h-4 text-muted-foreground" />
          <h2 className="text-sm font-semibold">Tip</h2>
        </div>
        <p className="text-sm text-muted-foreground">
          To see file columns across your tables, go to{' '}
          <strong className="text-foreground">Data</strong> and look for columns
          marked with the <Badge variant="secondary" className="text-[10px]">file</Badge> badge.
          Each file column can be configured as <Badge variant="secondary" className="text-[10px]">public</Badge> or{' '}
          <Badge variant="secondary" className="text-[10px]">private</Badge> during table creation.
        </p>
      </div>
    </div>
  )
}
