import { Settings } from 'lucide-react'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { useStore } from '@/state/tenantStore'

export default function ProjectSettingsPage() {
  const { projectId, projectName } = useStore()

  return (
    <div className="p-8 max-w-3xl mx-auto">
      <h1 className="text-2xl font-bold mb-1">Project Settings</h1>
      <p className="text-sm text-muted-foreground mb-8">{projectName}</p>
      <Card>
        <CardHeader className="flex flex-row items-center gap-3">
          <div className="flex items-center justify-center w-9 h-9 rounded-lg bg-muted">
            <Settings className="w-4 h-4 text-muted-foreground" />
          </div>
          <CardTitle className="text-base">Project ID</CardTitle>
        </CardHeader>
        <CardContent>
          <p className="font-mono text-sm text-muted-foreground">{projectId ?? '—'}</p>
        </CardContent>
      </Card>
    </div>
  )
}
