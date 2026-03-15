import FunctionDetailPage from '@/views/functions/FunctionDetailPage'
export function generateStaticParams() { return [{ functionId: '_' }] }
export default function Page() { return <FunctionDetailPage /> }
