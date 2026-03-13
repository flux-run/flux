import WorkflowsPage from '@/views/workflows/WorkflowsPage'

export function generateStaticParams() { return [{ projectId: "_projectId_" }] }
export default function Page() { return <WorkflowsPage /> }
