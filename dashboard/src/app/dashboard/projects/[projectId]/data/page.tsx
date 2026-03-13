import DatabasesPage from '@/views/data/DatabasesPage'

export function generateStaticParams() { return [{ projectId: "_projectId_" }] }
export default function Page() { return <DatabasesPage /> }
