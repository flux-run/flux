import FunctionsPage from '@/views/functions/FunctionsPage'

export function generateStaticParams() { return [{ projectId: "_projectId_" }] }
export default function Page() { return <FunctionsPage /> }
