import SchemaGraphPage from '@/views/schema/SchemaGraphPage'

export function generateStaticParams() { return [{ projectId: "_projectId_" }] }
export default function Page() { return <SchemaGraphPage /> }
