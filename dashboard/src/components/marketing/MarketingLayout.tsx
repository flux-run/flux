import { NavBar } from './NavBar'
import { Footer } from './Footer'

export function MarketingLayout({ children }: { children: React.ReactNode }) {
  return (
    <div className="marketing">
      <NavBar />
      <div style={{ paddingTop: 56 }}>
        {children}
      </div>
      <Footer />
    </div>
  )
}
