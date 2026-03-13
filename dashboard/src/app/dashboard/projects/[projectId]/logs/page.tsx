import LogsPage from '@/views/logs/LogsPage'

export function generateStaticParams() { return [{ projectId: "_projectId_" }] }
export default function Page() { return <LogsPage /> }
