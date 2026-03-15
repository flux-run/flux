import TableWorkspacePage from '@/views/data/TableWorkspacePage'
export function generateStaticParams() { return [{ database: '_', table: '_' }] }
export default function Page() { return <TableWorkspacePage /> }
