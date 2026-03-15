import TablesPage from '@/views/data/TablesPage'
export function generateStaticParams() { return [{ database: '_' }] }
export default function Page() { return <TablesPage /> }
