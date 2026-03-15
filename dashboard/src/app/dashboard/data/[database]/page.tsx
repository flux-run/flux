import TablesPage from '@/views/data/TablesPage'
export function generateStaticParams() { return [{ database: '_' }, { database: 'public' }, { database: 'flux' }] }
export default function Page() { return <TablesPage /> }
