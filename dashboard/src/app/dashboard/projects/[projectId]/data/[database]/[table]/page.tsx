import TableWorkspacePage from '@/views/data/TableWorkspacePage'

export function generateStaticParams() { return [{ projectId: "_projectId_", database: "_database_", table: "_table_" }] }
export default function Page() { return <TableWorkspacePage /> }
