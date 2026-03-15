'use client'

import { useState } from 'react'
import { useParams } from 'next/navigation'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { Globe, Plus, Trash2, Edit2, Copy, Check, Info } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { Badge } from '@/components/ui/badge'
import { Card, CardHeader, CardTitle, CardDescription, CardContent } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription, DialogFooter } from '@/components/ui/dialog'
import { apiFetch } from '@/lib/api'
import type { RouteRow, FunctionResponse } from '@flux/api-types'
import { useStore } from '@/state/tenantStore'
import { PageHeader } from '@/components/layout/PageHeader'

const METHODS = ['GET', 'POST', 'PUT', 'DELETE', 'PATCH']
const AUTH_TYPES = [
  { value: 'none', label: 'None (Public)' },
  { value: 'api_key', label: 'API Key' },
  { value: 'jwt', label: 'JWT (Auth0/Clerk/Firebase)' },
]

export default function RoutesPage() {
  const { projectId } = useParams() as any
  const { projectName } = useStore()
  const queryClient = useQueryClient()
  const [copiedId, setCopiedId] = useState<string | null>(null)
  const [createOpen, setCreateOpen] = useState(false)
  const [editingId, setEditingId] = useState<string | null>(null)
  
  // Form state
  const [method, setMethod] = useState('POST')
  const [path, setPath] = useState('/')
  const [functionId, setFunctionId] = useState('')
  const [authType, setAuthType] = useState('none')
  const [rateLimit, setRateLimit] = useState('100')
  const [corsEnabled, setCorsEnabled] = useState(false)
  const [corsOrigins, setCorsOrigins] = useState('')
  const [corsHeaders, setCorsHeaders] = useState('')
  const [jwksUrl, setJwksUrl] = useState('')
  const [jwtAudience, setJwtAudience] = useState('')
  const [jwtIssuer, setJwtIssuer] = useState('')
  const [jsonSchema, setJsonSchema] = useState('')

  // Fetch routes
  const { data: routes, isLoading } = useQuery({
    queryKey: ['projects', projectId, 'routes'],
    queryFn: async () => {
      const resp = await apiFetch<RouteRow[]>(`/routes?project_id=${projectId}`)
      return resp
    },
    enabled: !!projectId
  })

  // Fetch functions for the dropdown
  const { data: functionsData } = useQuery({
    queryKey: ['functions', projectId],
    queryFn: () => apiFetch<{ functions: FunctionResponse[] }>('/functions'),
    enabled: !!projectId
  })
  const functions = functionsData?.functions ?? []

  const copyToClipboard = (text: string, id: string) => {
    navigator.clipboard.writeText(text)
    setCopiedId(id)
    setTimeout(() => setCopiedId(null), 2000)
  }

  const createMutation = useMutation({
    mutationFn: async () => {
      const body = {
        method,
        path: path.startsWith('/') ? path : `/${path}`,
        function_id: functionId,
        auth_type: authType,
        cors_enabled: corsEnabled,
        rate_limit: rateLimit ? parseInt(rateLimit) : null,
        jwks_url: authType === 'jwt' && jwksUrl ? jwksUrl : null,
        jwt_audience: authType === 'jwt' && jwtAudience ? jwtAudience : null,
        jwt_issuer: authType === 'jwt' && jwtIssuer ? jwtIssuer : null,
        cors_origins: corsEnabled && corsOrigins ? corsOrigins.split(',').map(s => s.trim()) : null,
        cors_headers: corsEnabled && corsHeaders ? corsHeaders.split(',').map(s => s.trim()) : null,
        json_schema: jsonSchema ? JSON.parse(jsonSchema) : null,
      }
      
      if (editingId) {
        return apiFetch(`/routes/${editingId}`, {
          method: 'PATCH',
          body: JSON.stringify(body)
        })
      } else {
        return apiFetch('/routes', {
          method: 'POST',
          body: JSON.stringify(body)
        })
      }
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['projects', projectId, 'routes'] })
      setCreateOpen(false)
      resetForm()
    }
  })

  const deleteRoute = useMutation({
    mutationFn: async (id: string) => {
      return apiFetch(`/routes/${id}`, { method: 'DELETE' })
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['projects', projectId, 'routes'] })
    }
  })

  const resetForm = () => {
    setMethod('POST')
    setPath('/')
    setFunctionId('')
    setAuthType('none')
    setRateLimit('100')
    setCorsEnabled(false)
    setCorsOrigins('')
    setCorsHeaders('')
    setJwksUrl('')
    setJwtAudience('')
    setJwtIssuer('')
    setJsonSchema('')
    setEditingId(null)
  }

  const startEdit = (route: any) => {
    setMethod(route.method)
    setPath(route.path)
    setFunctionId(route.function_id)
    setAuthType(route.auth_type)
    setRateLimit(route.rate_limit?.toString() || '')
    setCorsEnabled(route.cors_enabled || false)
    setCorsOrigins(route.cors_origins?.join(', ') || '')
    setCorsHeaders(route.cors_headers?.join(', ') || '')
    setJwksUrl(route.jwks_url || '')
    setJwtAudience(route.jwt_audience || '')
    setJwtIssuer(route.jwt_issuer || '')
    setJsonSchema(route.json_schema ? JSON.stringify(route.json_schema, null, 2) : '')
    setEditingId(route.id)
    setCreateOpen(true)
  }

  const getBaseDomain = () => 'localhost:4000'

  const getFullUrl = (routePath: string) => {
    const cleanPath = routePath.startsWith('/') ? routePath : `/${routePath}`
    return `https://${getBaseDomain()}${cleanPath}`
  }

  const publicUrl = getFullUrl(path)

  if (isLoading) return <div className="p-8 animate-pulse text-muted-foreground">Loading routes...</div>

  return (
    <div className="flex flex-col h-full">
      <PageHeader
        title="API Routes"
        description={routes && routes.length > 0 ? `${routes.length} route${routes.length !== 1 ? 's' : ''}` : 'Public HTTP endpoints mapped to functions'}
        breadcrumbs={[
          { label: 'Projects', href: '/dashboard' },
          { label: projectName ?? projectId ?? '…', href: `/dashboard/projects/${projectId}/overview` },
          { label: 'Routes' },
        ]}
        actions={
          <Button size="sm" className="gap-2" onClick={() => { resetForm(); setCreateOpen(true); }}>
            <Plus className="w-3.5 h-3.5" /> Create route
          </Button>
        }
      />
      <div className="flex-1 overflow-y-auto">
      <div className="p-6 space-y-6 max-w-6xl mx-auto">

      <Card>
        <CardContent className="p-0">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead className="w-24">Method</TableHead>
                <TableHead>Path</TableHead>
                <TableHead>Target Function</TableHead>
                <TableHead>Security</TableHead>
                <TableHead>Rate Limit</TableHead>
                <TableHead className="text-right">Actions</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {routes?.length === 0 ? (
                <TableRow>
                  <TableCell colSpan={6} className="h-32 text-center text-muted-foreground">
                    No routes found. Create your first API route to get started.
                  </TableCell>
                </TableRow>
              ) : (
                routes?.map((route) => {
                  const targetFn = functions.find(f => f.id === route.function_id)
                  return (
                    <TableRow key={route.id} className="group">
                      <TableCell>
                        <Badge variant="outline" className="font-mono uppercase text-[10px] py-0">
                          {route.method}
                        </Badge>
                      </TableCell>
                      <TableCell className="font-mono text-sm max-w-xs truncate">
                        <div className="flex items-center gap-2">
                          <span className="text-muted-foreground/50">/</span>
                          {route.path.replace(/^\//, '')}
                          <button 
                            onClick={() => copyToClipboard(getFullUrl(route.path), route.id)}
                            className="opacity-0 group-hover:opacity-100 transition-opacity p-1 hover:bg-white/5 rounded"
                            title="Copy Public URL"
                          >
                            {copiedId === route.id ? (
                              <Check className="w-3 h-3 text-green-500" />
                            ) : (
                              <Copy className="w-3 h-3 text-muted-foreground" />
                            )}
                          </button>
                        </div>
                      </TableCell>
                      <TableCell>
                        <div className="flex items-center gap-2">
                          <Badge variant="secondary" className="font-normal text-xs bg-primary/5 text-primary border-primary/10">
                            {targetFn?.name || route.function_id.slice(0, 8)}
                          </Badge>
                        </div>
                      </TableCell>
                      <TableCell>
                        <Badge 
                          variant={route.auth_type === 'none' ? 'secondary' : 'default'}
                          className="capitalize text-[10px] py-0"
                        >
                          {route.auth_type === 'none' ? 'Public' : route.auth_type.replace('_', ' ')}
                        </Badge>
                      </TableCell>
                      <TableCell className="text-muted-foreground text-xs font-mono">
                        {route.rate_limit ? `${route.rate_limit}/min` : '∞'}
                      </TableCell>
                      <TableCell className="text-right">
                        <div className="flex justify-end gap-1 opacity-0 group-hover:opacity-100 transition-opacity">
                          <Button 
                            variant="ghost" 
                            size="icon" 
                            className="h-8 w-8"
                            onClick={() => startEdit(route)}
                          >
                            <Edit2 className="w-3.5 h-3.5" />
                          </Button>
                          <Button 
                            variant="ghost" 
                            size="icon" 
                            className="h-8 w-8 text-destructive hover:text-destructive"
                            onClick={() => {
                              if (confirm('Are you sure you want to delete this route?')) {
                                deleteRoute.mutate(route.id)
                              }
                            }}
                          >
                            <Trash2 className="w-3.5 h-3.5" />
                          </Button>
                        </div>
                      </TableCell>
                    </TableRow>
                  )
                })
              )}
            </TableBody>
          </Table>
        </CardContent>
      </Card>
      
      <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
        <Card className="bg-muted/30">
          <CardHeader className="pb-3">
            <CardTitle className="text-sm font-medium">Public Endpoint</CardTitle>
            <CardDescription>Public base URL for this project.</CardDescription>
          </CardHeader>
          <CardContent>
            <div className="flex items-center gap-2 p-3 bg-background rounded-lg border font-mono text-xs overflow-hidden">
              <span className="text-primary shrink-0">https://</span>
              <span className="truncate">{getBaseDomain()}</span>
              <Button variant="ghost" size="icon" className="ml-auto h-7 w-7 shrink-0" onClick={() => copyToClipboard(`https://${getBaseDomain()}`, 'base')}>
                <Copy className="w-3.5 h-3.5" />
              </Button>
            </div>
          </CardContent>
        </Card>
        
        <Card className="bg-primary/5 border-primary/20">
          <CardHeader className="pb-3">
            <CardTitle className="text-sm font-medium flex items-center gap-2">
              <Globe className="w-4 h-4 text-primary" />
              Developer Guide
            </CardTitle>
          </CardHeader>
          <CardContent className="text-xs text-muted-foreground leading-relaxed">
            Routes map HTTP requests to your functions. Use <strong>API Key</strong> auth for server-to-server calls, or <strong>Public</strong> for webhooks. Rate limits protect your project from excessive usage.
          </CardContent>
        </Card>
      </div>

      <Dialog open={createOpen} onOpenChange={(open) => { if(!open) resetForm(); setCreateOpen(open); }}>
        <DialogContent className="sm:max-w-[500px]">
          <DialogHeader>
            <DialogTitle>{editingId ? 'Edit Gateway Route' : 'Create Gateway Route'}</DialogTitle>
            <DialogDescription>Map a public URL to one of your functions.</DialogDescription>
          </DialogHeader>
          
          <div className="space-y-5 py-4">
            <div className="grid grid-cols-4 items-center gap-4">
              <Label className="text-right">Method</Label>
              <div className="col-span-3">
                <Select value={method} onValueChange={setMethod}>
                  <SelectTrigger>
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {METHODS.map(m => <SelectItem key={m} value={m}>{m}</SelectItem>)}
                  </SelectContent>
                </Select>
              </div>
            </div>

            <div className="grid grid-cols-4 items-center gap-4">
              <Label className="text-right">Path</Label>
              <div className="col-span-3 relative">
                <span className="absolute left-3 top-1/2 -translate-y-1/2 text-muted-foreground font-mono text-sm">/</span>
                <Input 
                  className="pl-6 font-mono"
                  placeholder="v1/hello"
                  value={path.startsWith('/') ? path.slice(1) : path}
                  onChange={(e) => setPath('/' + e.target.value)}
                />
              </div>
            </div>

            <div className="grid grid-cols-4 items-center gap-4">
              <Label className="text-right">Function</Label>
              <div className="col-span-3">
                <Select value={functionId} onValueChange={setFunctionId}>
                  <SelectTrigger>
                    <SelectValue placeholder="Select target function" />
                  </SelectTrigger>
                  <SelectContent>
                    {functions.map(fn => (
                      <SelectItem key={fn.id} value={fn.id}>{fn.name}</SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
            </div>

            <div className="grid grid-cols-4 items-center gap-4">
              <Label className="text-right">Security</Label>
              <div className="col-span-3">
                <Select value={authType} onValueChange={setAuthType}>
                  <SelectTrigger>
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {AUTH_TYPES.map(a => <SelectItem key={a.value} value={a.value}>{a.label}</SelectItem>)}
                  </SelectContent>
                </Select>
              </div>
            </div>

            {authType === 'jwt' && (
              <div className="bg-muted/30 p-4 rounded-lg space-y-4 text-sm border">
                <div className="font-semibold text-xs uppercase tracking-wider text-muted-foreground mb-2">JWT Configuration</div>
                <div className="grid grid-cols-4 items-center gap-4">
                  <Label className="text-right text-xs">JWKS URL *</Label>
                  <div className="col-span-3">
                    <Input placeholder="https://YOUR_DOMAIN/.well-known/jwks.json" value={jwksUrl} onChange={(e) => setJwksUrl(e.target.value)} />
                  </div>
                </div>
                <div className="grid grid-cols-4 items-center gap-4">
                  <Label className="text-right text-xs">Audience</Label>
                  <div className="col-span-3">
                    <Input placeholder="Optional API Audience" value={jwtAudience} onChange={(e) => setJwtAudience(e.target.value)} />
                  </div>
                </div>
                <div className="grid grid-cols-4 items-center gap-4">
                  <Label className="text-right text-xs">Issuer</Label>
                  <div className="col-span-3">
                    <Input placeholder="Optional Token Issuer" value={jwtIssuer} onChange={(e) => setJwtIssuer(e.target.value)} />
                  </div>
                </div>
              </div>
            )}

            <div className="grid grid-cols-4 items-center gap-4">
              <Label className="text-right">Rate Limit</Label>
              <div className="col-span-3 flex items-center gap-3">
                <Input 
                  type="number"
                  className="w-24 font-mono"
                  placeholder="100"
                  value={rateLimit}
                  onChange={(e) => setRateLimit(e.target.value)}
                />
                <span className="text-xs text-muted-foreground">req/minute</span>
              </div>
            </div>

            <div className="grid grid-cols-4 items-center gap-4">
              <Label className="text-right">CORS</Label>
              <div className="col-span-3 flex items-center space-x-2">
                <input 
                  type="checkbox" 
                  id="cors" 
                  className="rounded border-gray-300 text-primary focus:ring-primary h-4 w-4"
                  checked={corsEnabled}
                  onChange={(e) => setCorsEnabled(e.target.checked)}
                />
                <label htmlFor="cors" className="text-sm font-medium leading-none">
                  Enable Advanced CORS Handling
                </label>
              </div>
            </div>

            {corsEnabled && (
               <div className="bg-muted/30 p-4 rounded-lg space-y-4 text-sm border">
               <div className="grid grid-cols-4 items-center gap-4">
                 <Label className="text-right text-xs">Origins</Label>
                 <div className="col-span-3">
                   <Input placeholder="https://app.acme.com (comma separated)" value={corsOrigins} onChange={(e) => setCorsOrigins(e.target.value)} />
                 </div>
               </div>
               <div className="grid grid-cols-4 items-center gap-4">
                 <Label className="text-right text-xs">Headers</Label>
                 <div className="col-span-3">
                   <Input placeholder="Content-Type, Authorization" value={corsHeaders} onChange={(e) => setCorsHeaders(e.target.value)} />
                 </div>
               </div>
             </div>
            )}

            <div className="grid grid-cols-4 items-start gap-4">
              <Label className="text-right mt-2">Zod Schema <br/><span className="text-[10px] text-muted-foreground font-normal">(JSON Schema)</span></Label>
              <div className="col-span-3">
                <textarea 
                  className="flex min-h-[80px] w-full rounded-md border border-input bg-background px-3 py-2 text-sm ring-offset-background placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 disabled:cursor-not-allowed disabled:opacity-50 font-mono"
                  placeholder='{"type": "object", "properties": {"email": {"type": "string"}}}'
                  value={jsonSchema}
                  onChange={e => setJsonSchema(e.target.value)}
                />
              </div>
            </div>

            <div className="bg-muted/50 p-4 rounded-lg border space-y-2">
              <div className="flex items-center gap-2 text-[10px] font-bold uppercase tracking-wider text-muted-foreground">
                <Globe className="w-3 h-3" />
                Live Route Preview
              </div>
              <div className="font-mono text-[11px] truncate text-primary">
                {method} {publicUrl}
              </div>
              {authType === 'api_key' && (
                <div className="text-[10px] text-muted-foreground flex items-center gap-1">
                  <Info className="w-3 h-3" /> Requires X-API-Key header
                </div>
              )}
              {authType === 'jwt' && (
                <div className="text-[10px] text-muted-foreground flex items-center gap-1">
                  <Info className="w-3 h-3" /> Requires Bearer Token
                </div>
              )}
            </div>
          </div>

          <DialogFooter>
            <Button variant="outline" onClick={() => setCreateOpen(false)}>Cancel</Button>
            <Button 
              onClick={() => createMutation.mutate()} 
              disabled={!functionId || !path || createMutation.isPending}
            >
              {createMutation.isPending ? 'Creating...' : 'Create Route'}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
      </div>
      </div>
    </div>
  )
}
