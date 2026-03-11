'use client'

import { useState } from 'react'

interface CodeWindowProps {
  label: string
  children: string
}

export function CodeWindow({ label, children }: CodeWindowProps) {
  const [copied, setCopied] = useState(false)

  const handleCopy = () => {
    // Strip ANSI-style span tags, get plain text
    const div = document.createElement('div')
    div.innerHTML = children
    const text = div.innerText || div.textContent || ''
    navigator.clipboard.writeText(text).then(() => {
      setCopied(true)
      setTimeout(() => setCopied(false), 1800)
    })
  }

  return (
    <div style={{
      background: '#0a0a0c',
      border: '1px solid var(--mg-border)',
      borderRadius: 10,
      overflow: 'hidden',
      textAlign: 'left',
    }}>
      {/* Title bar */}
      <div style={{
        background: 'var(--mg-bg-elevated)',
        borderBottom: '1px solid var(--mg-border)',
        padding: '10px 16px',
        display: 'flex', alignItems: 'center', gap: 8,
      }}>
        <span style={{ width: 10, height: 10, borderRadius: '50%', background: '#f87171', display: 'inline-block', flexShrink: 0 }} />
        <span style={{ width: 10, height: 10, borderRadius: '50%', background: 'var(--mg-yellow)', display: 'inline-block', flexShrink: 0 }} />
        <span style={{ width: 10, height: 10, borderRadius: '50%', background: 'var(--mg-green)', display: 'inline-block', flexShrink: 0 }} />
        <span style={{ fontSize: '.75rem', color: 'var(--mg-muted)', marginLeft: 8, fontFamily: 'var(--font-geist-mono, monospace)', flex: 1 }}>
          {label}
        </span>
        <button
          onClick={handleCopy}
          style={{
            background: 'none',
            border: '1px solid var(--mg-border)',
            borderRadius: 4,
            color: copied ? 'var(--mg-green)' : 'var(--mg-muted)',
            fontSize: '.68rem',
            padding: '2px 8px',
            cursor: 'pointer',
            fontFamily: 'inherit',
            transition: 'color .15s, border-color .15s',
          }}
        >
          {copied ? 'Copied!' : 'Copy'}
        </button>
      </div>
      {/* Code body — rendered as HTML since it uses inline spans for color */}
      <pre style={{
        margin: 0,
        padding: '24px 28px',
        fontFamily: 'var(--font-geist-mono, "JetBrains Mono", monospace)',
        fontSize: '.82rem',
        lineHeight: 1.85,
        overflowX: 'auto',
        whiteSpace: 'pre-wrap',
        wordBreak: 'break-word',
      }}>
        <code dangerouslySetInnerHTML={{ __html: children }} />
      </pre>
    </div>
  )
}
