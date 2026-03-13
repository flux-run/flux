import AgentsPage from '@/views/agents/AgentsPage'

export function generateStaticParams() { return [{ projectId: "_projectId_" }] }
export default function Page() { return <AgentsPage /> }
