import type { Metadata } from 'next'

export const metadata: Metadata = {
  title: 'Install — Fluxbase Docs',
  description: '',
}

export default function Page() {
  return (
    <div
      dangerouslySetInnerHTML={{ __html: `<h1>Install the Fluxbase CLI</h1>
<p class="page-subtitle">One command installs the <code>flux</code> binary on macOS, Linux, or Windows (arm64 and amd64).</p>

<!-- One-liner -->
<h2>One-command install</h2>
<p>The fastest way to install on macOS or Linux:</p>
<pre><code><span class="shell-prompt">$</span> curl -fsSL https://fluxbase.co/install | bash</code></pre>

<div class="callout callout-info">
  <div class="callout-title">What the script does</div>
  Detects your OS and CPU architecture, downloads the correct binary from the latest GitHub Release,
  and moves it to <code>/usr/local/bin/flux</code> (or <code>~/.local/bin/flux</code> if
  <code>/usr/local/bin</code> is not writable).
</div>

<!-- macOS -->
<h2>macOS</h2>
<h3>Automatic (recommended)</h3>
<pre><code><span class="shell-prompt">$</span> curl -fsSL https://fluxbase.co/install | bash</code></pre>

<h3>Homebrew (coming soon)</h3>
<pre><code><span class="shell-prompt">$</span> brew install fluxbase/tap/flux</code></pre>

<h3>Manual download</h3>
<pre><code><span class="cm"># Apple Silicon (M1/M2/M3/M4)</span>
<span class="shell-prompt">$</span> curl -fsSL https://github.com/fluxbase/fluxbase/releases/latest/download/flux-darwin-arm64 \\
    -o /usr/local/bin/flux &amp;&amp; chmod +x /usr/local/bin/flux

<span class="cm"># Intel Mac</span>
<span class="shell-prompt">$</span> curl -fsSL https://github.com/fluxbase/fluxbase/releases/latest/download/flux-darwin-amd64 \\
    -o /usr/local/bin/flux &amp;&amp; chmod +x /usr/local/bin/flux</code></pre>

<!-- Linux -->
<h2>Linux</h2>
<h3>Automatic</h3>
<pre><code><span class="shell-prompt">$</span> curl -fsSL https://fluxbase.co/install | bash</code></pre>

<h3>Manual download</h3>
<pre><code><span class="cm"># x86_64 / amd64</span>
<span class="shell-prompt">$</span> curl -fsSL https://github.com/fluxbase/fluxbase/releases/latest/download/flux-linux-amd64 \\
    -o /usr/local/bin/flux &amp;&amp; chmod +x /usr/local/bin/flux

<span class="cm"># ARM64 (Graviton, Raspberry Pi, etc.)</span>
<span class="shell-prompt">$</span> curl -fsSL https://github.com/fluxbase/fluxbase/releases/latest/download/flux-linux-arm64 \\
    -o /usr/local/bin/flux &amp;&amp; chmod +x /usr/local/bin/flux</code></pre>

<!-- Windows -->
<h2>Windows</h2>
<h3>Direct download</h3>
<p>Download the binary for your architecture and add it to a directory in your <code>PATH</code>:</p>
<ul>
  <li><a href="https://github.com/fluxbase/fluxbase/releases/latest/download/flux-windows-amd64.exe">flux-windows-amd64.exe</a> — Intel/AMD 64-bit</li>
  <li><a href="https://github.com/fluxbase/fluxbase/releases/latest/download/flux-windows-arm64.exe">flux-windows-arm64.exe</a> — ARM64 (Snapdragon, etc.)</li>
</ul>

<h3>Scoop (coming soon)</h3>
<pre><code>scoop bucket add fluxbase https://github.com/fluxbase/scoop
scoop install flux</code></pre>

<!-- Verify -->
<h2>Verify installation</h2>
<pre><code><span class="shell-prompt">$</span> flux --version
flux 0.1.0</code></pre>

<h2>Authenticate</h2>
<p>Once installed, log in to your Fluxbase account:</p>
<pre><code><span class="shell-prompt">$</span> flux login</code></pre>
<p>This opens a browser window, signs you in with Google, and saves a token to <code>~/.config/fluxbase/config.toml</code>.</p>

<div class="callout callout-success">
  <div class="callout-title">You're ready!</div>
  Head to the <a href="/docs/quickstart">Quickstart</a> to deploy your first function.
</div>

<!-- Platform support table -->
<h2>Supported platforms</h2>
<table>
  <thead><tr><th>OS</th><th>Architecture</th><th>Binary</th></tr></thead>
  <tbody>
    <tr><td>macOS</td><td>arm64 (Apple Silicon)</td><td><code>flux-darwin-arm64</code></td></tr>
    <tr><td>macOS</td><td>amd64 (Intel)</td><td><code>flux-darwin-amd64</code></td></tr>
    <tr><td>Linux</td><td>arm64</td><td><code>flux-linux-arm64</code></td></tr>
    <tr><td>Linux</td><td>amd64</td><td><code>flux-linux-amd64</code></td></tr>
    <tr><td>Windows</td><td>arm64</td><td><code>flux-windows-arm64.exe</code></td></tr>
    <tr><td>Windows</td><td>amd64</td><td><code>flux-windows-amd64.exe</code></td></tr>
  </tbody>
</table>

<hr>
<p style="display:flex;gap:16px;font-size:.875rem;">
  <a href="/docs/quickstart">Next: Quickstart →</a>
</p>` }}
    />
  )
}
