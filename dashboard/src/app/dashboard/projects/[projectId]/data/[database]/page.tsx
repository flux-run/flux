import TablesPage from '@/views/data/TablesPage'

export function generateStaticParams() { return [{ projectId: "_projectId_", database: "_database_" }] }
export default function Page() { return <TablesPage /> }
