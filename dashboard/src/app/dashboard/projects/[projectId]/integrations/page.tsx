import IntegrationsPage from '@/views/integrations/IntegrationsPage'

export function generateStaticParams() { return [{ projectId: "_projectId_" }] }
export default function Page() { return <IntegrationsPage /> }
