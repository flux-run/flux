/**
 * Section-level building blocks.
 */

/**
 * Eyebrow label above a section heading.
 * @param {{ text: string, color?: 'accent' | 'green' | 'muted' }} props
 */
export function eyebrow({ text, color = 'accent' } = {}) {
  const styles = {
    accent: 'color:var(--accent);background:var(--accent-dim);',
    green:  'color:var(--green);background:rgba(61,214,140,.1);',
    muted:  'color:var(--muted);background:var(--bg-elevated);',
  };
  return `<div style="display:inline-block;font-size:.72rem;font-weight:700;letter-spacing:.1em;text-transform:uppercase;${styles[color]}padding:4px 12px;border-radius:20px;margin-bottom:20px;">${text}</div>`;
}

/**
 * Section heading + optional subtext block.
 * @param {{ heading: string, sub?: string, maxWidth?: string }} props
 */
export function sectionHeader({ heading, sub = '', maxWidth = '560px' } = {}) {
  return `<h2 style="font-size:clamp(1.4rem,3vw,2rem);font-weight:800;letter-spacing:-.03em;margin-bottom:${sub ? '10px' : '40px'};">${heading}</h2>
${sub ? `<p style="color:var(--muted);font-size:.95rem;max-width:${maxWidth};margin:0 0 40px;">${sub}</p>` : ''}`;
}

/**
 * Full-width section wrapper.
 * @param {{ id?: string, bg?: string, border?: boolean, padding?: string, content: string }} props
 */
export function section({ id = '', bg = '', border = true, padding = '80px 0', content = '' } = {}) {
  const style = [
    border ? 'border-top:1px solid var(--border);' : '',
    `padding:${padding};`,
    bg ? `background:${bg};` : '',
  ].filter(Boolean).join(' ');

  return `<section${id ? ` id="${id}"` : ''} style="${style}">
<div style="max-width:1040px;margin:0 auto;padding:0 24px;">
${content}
</div>
</section>`;
}

/**
 * Feature card.
 * @param {{ icon: string, title: string, badge?: string, body: string }} props
 */
export function featureCard({ icon, title, badge = '', body } = {}) {
  return `<div class="feature-card">
  <div class="icon">${icon}</div>
  <h3>${title}${badge ? ` <span style="font-size:.72rem;font-family:var(--font-mono);color:var(--accent);font-weight:400;margin-left:6px;">${badge}</span>` : ''}</h3>
  <p>${body}</p>
</div>`;
}

/**
 * Two-column comparison block.
 * @param {{ leftTitle: string, leftItems: string[], rightTitle: string, rightItems: string[], rightIsAccent?: boolean }} props
 */
export function comparisonGrid({ leftTitle, leftItems, rightTitle, rightItems, rightIsAccent = true } = {}) {
  const bad = leftItems.map(i =>
    `<div style="display:flex;align-items:center;gap:12px;font-size:.88rem;color:var(--muted);"><span style="color:var(--red);">✗</span> ${i}</div>`
  ).join('\n        ');

  const good = rightItems.map(i =>
    `<div style="display:flex;align-items:center;gap:12px;font-size:.88rem;color:var(--muted);"><span style="color:var(--green);">✓</span> ${i}</div>`
  ).join('\n        ');

  const rightBorder = rightIsAccent
    ? 'border:1px solid var(--accent);'
    : 'border:1px solid var(--border);';
  const rightHeader = rightIsAccent
    ? 'background:var(--accent-dim);border-bottom:1px solid var(--accent);'
    : 'background:var(--bg-elevated);border-bottom:1px solid var(--border);';
  const rightTitleColor = rightIsAccent ? 'color:var(--accent);' : 'color:var(--text);';

  return `<div class="grid-2col" style="display:grid;grid-template-columns:1fr 1fr;gap:48px;align-items:start;">
  <div style="border:1px solid var(--border);border-radius:10px;overflow:hidden;">
    <div style="background:var(--bg-elevated);border-bottom:1px solid var(--border);padding:14px 20px;">
      <span style="font-size:.8rem;font-weight:700;color:var(--muted);text-transform:uppercase;letter-spacing:.06em;">${leftTitle}</span>
    </div>
    <div style="padding:24px 20px;display:flex;flex-direction:column;gap:10px;">
      ${bad}
    </div>
  </div>

  <div style="${rightBorder}border-radius:10px;overflow:hidden;">
    <div style="${rightHeader}padding:14px 20px;">
      <span style="font-size:.8rem;font-weight:700;${rightTitleColor}text-transform:uppercase;letter-spacing:.06em;">${rightTitle}</span>
    </div>
    <div style="padding:24px 20px;display:flex;flex-direction:column;gap:10px;">
      ${good}
    </div>
  </div>
</div>`;
}
