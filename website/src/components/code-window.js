/**
 * macOS-style code / terminal window chrome.
 *
 * @param {{ title?: string, content: string, maxWidth?: string }} props
 *   title   — text shown in the title bar (e.g. "flux why 550e8400")
 *   content — pre-formatted HTML/text for the body (already escaped if needed)
 *   maxWidth — CSS max-width for the window wrapper; defaults to '720px'
 */
let _cwIdx = 0;
export function codeWindow({ title = '', content = '', maxWidth = '720px' } = {}) {
  const id = `cw-${++_cwIdx}`;
  // Strip HTML tags to get plain text for copy
  const copyScript = `(function(){
    var el=document.getElementById('${id}');
    var txt=el.innerText||el.textContent;
    navigator.clipboard.writeText(txt).then(function(){
      var btn=document.getElementById('${id}-copy');
      btn.textContent='Copied!';
      btn.style.color='var(--green)';
      setTimeout(function(){btn.textContent='Copy';btn.style.color='';},1800);
    });
  })()`;

  return `<div style="max-width:${maxWidth};margin:0 auto;background:#0a0a0c;border:1px solid var(--border);border-radius:10px;overflow:hidden;text-align:left;">
  <div style="background:var(--bg-elevated);border-bottom:1px solid var(--border);padding:10px 16px;display:flex;align-items:center;gap:8px;">
    <span style="width:10px;height:10px;border-radius:50%;background:#f87171;display:inline-block;flex-shrink:0;"></span>
    <span style="width:10px;height:10px;border-radius:50%;background:var(--yellow);display:inline-block;flex-shrink:0;"></span>
    <span style="width:10px;height:10px;border-radius:50%;background:var(--green);display:inline-block;flex-shrink:0;"></span>
    ${title ? `<span style="font-size:.75rem;color:var(--muted);margin-left:8px;font-family:var(--font-mono);flex:1;">${title}</span>` : '<span style="flex:1;"></span>'}
    <button id="${id}-copy" onclick="${copyScript.replace(/"/g, '&quot;')}" style="background:none;border:1px solid var(--border);border-radius:4px;color:var(--muted);font-size:.68rem;padding:2px 8px;cursor:pointer;font-family:var(--font);transition:color .15s,border-color .15s;" onmouseenter="this.style.borderColor='var(--accent)';this.style.color='var(--text)'" onmouseleave="this.style.borderColor='var(--border)';this.style.color='var(--muted)'">Copy</button>
  </div>
  <pre id="${id}" style="margin:0;padding:24px 28px;font-family:var(--font-mono);font-size:.82rem;line-height:1.85;overflow-x:auto;white-space:pre-wrap;word-break:break-word;"><code>${content}</code></pre>
</div>`;
}

/**
 * Inline code helpers — return coloured span strings for terminal output.
 */
export const c = {
  cmd:    (t) => `<span style="color:var(--green);">${t}</span>`,
  id:     (t) => `<span style="color:var(--accent);">${t}</span>`,
  dim:    (t) => `<span style="color:var(--muted);">${t}</span>`,
  fn:     (t) => `<span style="color:#f9a8d4;">${t}</span>`,
  db:     (t) => `<span style="color:#60a5fa;">${t}</span>`,
  ms:     (t) => `<span style="color:var(--yellow);">${t}</span>`,
  err:    (t) => `<span style="color:var(--red);">${t}</span>`,
  ok:     (t) => `<span style="color:var(--green);">${t}</span>`,
  purple: (t) => `<span style="color:#a78bfa;">${t}</span>`,
  white:  (t) => `<span style="color:#f8f8f2;">${t}</span>`,
};
