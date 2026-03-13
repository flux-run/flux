import QueryExplorerPage from '@/views/query/QueryExplorerPage'

export function generateStaticParams() { return [{ projectId: "_projectId_" }] }
export default function Page() { return <QueryExplorerPage /> }
