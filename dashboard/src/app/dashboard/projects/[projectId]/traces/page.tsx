import TracesPage from '@/views/traces/TracesPage'

export function generateStaticParams() { return [{ projectId: "_projectId_" }] }
export default function Page() { return <TracesPage /> }
