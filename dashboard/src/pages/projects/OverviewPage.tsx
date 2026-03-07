import { useQuery } from '@tanstack/react-query'
import { useParams } from 'react-router-dom'
import { Code2, ShieldCheck, KeyRound, Layers } from 'lucide-react'
import { apiFetch } from '@/lib/api'
import { useStore } from '@/state/tenantStore'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'

interface Fn { id: string; name: string; runtime: string }
interface Secret { id: string; key: string }

export default function OverviewPage() {
  const { projectId: paramId } = useParams<{ projectId: string }>()
  const { projectId: storeId, projectName } = useStore()
  const projectId = paramId ?? storeId

  const fns = useQuery({
    queryKey: ['functions', projectId],
    queryFn: () => apiFetch<{ functions: Fn[] }>('/functions'),
    enabled: !!projectId,
  })

  const secrets = useQuery({
    queryKey: ['secrets', projectId],
    queryFn: () => apiFetch<{ secrets: Secret[] }>('/secrets'),
    enabled: !!projectId,
  })

  const stats = [
    {
      label: 'Functions',
      value: fns.data?.functions.length ?? '—',
      icon: Code2,
      color: 'text-violet-400',
      bg: 'bg-violet-500/10',
    },
    {
      label: 'Secrets',
      value: secrets.data?.secrets.length ?? '—',
      icon: ShieldCheck,
      color: 'text-emerald-400',
      bg: 'bg-emerald-500/10',
    },
    {
      label: 'Deployments',
      value: fns.data?.functions.length != null ? fns.data.functions.length * 1 : '—',
      icon: Layers,
      color: 'text-blue-400',
      bg: 'bg-blue-500/10',
    },
    {
      label: 'API Keys',
      value: '—',
      icon: KeyRound,
      color: 'text-amber-400',
      bg: 'bg-amber-500/10',
    },
  ]

  return (
    <div className="p-8 max-w-5xl mx-auto">
      <div className="mb-8">
        <h1 className="text-2xl font-bold">{projectName ?? 'Project overview'}</h1>
        <p className="text-sm text-muted-foreground mt-0.5">
          Summary of your project resources
        </p>
      </div>

      <div className="grid grid-cols-2 lg:grid-cols-4 gap-4 mb-8">
        {stats.map((s) => (
          <Card key={s.label}>
            <CardHeader className="flex flex-row items-center justify-between pb-2">
              <CardTitle className="text-sm font-medium text-muted-foreground">
                {s.label}
              </CardTitle>
              <div className={`p-1.5 rounded-lg ${s.bg}`}>
                <s.icon className={`w-3.5 h-3.5 ${s.color}`} />
              </div>
            </CardHeader>
            <CardContent>
              <p className="text-2xl font-bold">{s.value}</p>
            </CardContent>
          </Card>
        ))}
      </div>

      {/* Recent functions */}
      <Card>
        <CardHeader>
          <CardTitle className="text-base">Recent functions</CardTitle>
        </CardHeader>
        <CardContent>
          {!fns.data?.functions.length ? (
            <p className="text-sm text-muted-foreground">No functions yet.</p>
          ) : (
            <div className="space-y-2">
              {fns.data.functions.slice(0, 5).map((fn) => (
                <div key={fn.id} className="flex items-center justify-between py-1.5 border-b last:border-0">
                  <div>
                    <p className="text-sm font-medium">{fn.name}</p>
                    <p className="text-xs text-muted-foreground">{fn.runtime}</p>
                  </div>
                  <span className="text-xs text-muted-foreground font-mono truncate max-w-[120px]">{fn.id}</span>
                </div>
              ))}
            </div>
          )}
        </CardContent>
      </Card>
    </div>
  )
}
