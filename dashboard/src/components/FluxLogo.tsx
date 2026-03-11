/**
 * FluxLogo — shared brand identity components used across
 * marketing pages, the dashboard sidebar, and any other surface.
 *
 * Brand accent: #6c63ff  (mapped to --brand-accent globally)
 */

/** Standalone brand icon — rounded-rect background with a bolt mark. */
export function FluxIcon({
  size = 28,
  className,
}: {
  size?: number
  className?: string
}) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 28 28"
      fill="none"
      xmlns="http://www.w3.org/2000/svg"
      className={className}
      aria-hidden="true"
    >
      <rect width="28" height="28" rx="7" fill="#6c63ff" />
      {/* Lightning-bolt mark, centered within 4 px margin */}
      <path
        d="M15.5 4L9 15H13.5L12.5 24L19 13.5H14.5L15.5 4Z"
        fill="white"
      />
    </svg>
  )
}

/**
 * Wordmark — "flux" in brand purple, "base" inherits parent color.
 * Works on both dark (sidebar, marketing) and light (dashboard header) BGs.
 */
export function FluxWordmark({
  fontSize = 14,
  baseColor = 'inherit',
}: {
  fontSize?: number
  baseColor?: string
}) {
  return (
    <span
      style={{
        fontWeight: 700,
        fontSize,
        letterSpacing: '-0.02em',
        lineHeight: 1,
        userSelect: 'none',
      }}
    >
      <span style={{ color: '#6c63ff' }}>flux</span>
      <span style={{ color: baseColor }}>base</span>
    </span>
  )
}

/** Icon + wordmark side-by-side. */
export function FluxLogo({
  iconSize = 28,
  fontSize = 14,
  gap = 8,
  baseColor = 'inherit',
  className,
}: {
  iconSize?: number
  fontSize?: number
  gap?: number
  baseColor?: string
  className?: string
}) {
  return (
    <span
      className={className}
      style={{ display: 'inline-flex', alignItems: 'center', gap }}
    >
      <FluxIcon size={iconSize} />
      <FluxWordmark fontSize={fontSize} baseColor={baseColor} />
    </span>
  )
}
