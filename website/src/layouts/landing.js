/**
 * Landing page layout (no sidebar).
 * Wraps content in the standard HTML shell + topnav.
 */
import { nav }    from '../components/nav.js';
import { footer } from '../components/footer.js';

/**
 * @param {{ meta: { title: string, description: string }, active?: string, content: string, extraHead?: string }} props
 */
export function landingLayout({ meta, active = '', content = '', extraHead = '' } = {}) {
  const FONTS = `<link rel="preconnect" href="https://fonts.googleapis.com">
  <link href="https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600;700;800&family=JetBrains+Mono&display=swap" rel="stylesheet">`;

  return `<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width,initial-scale=1">
  <title>${meta.title}</title>
  <meta name="description" content="${meta.description}">
  ${FONTS}
  <link rel="stylesheet" href="/assets/style.css">
  ${extraHead}
</head>
<body>

${nav({ active })}

<div class="landing-wrap">
${content}
${footer()}
</div>

</body>
</html>`;
}
