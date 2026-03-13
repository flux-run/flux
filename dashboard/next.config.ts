import type { NextConfig } from "next";

const nextConfig: NextConfig = {
  basePath: "/flux",
  // Static export — builds to `out/` for embedding in the Rust server.
  // The Rust server serves the SPA with an index.html fallback for unknown
  // dashboard paths, so client-side navigation works after a hard refresh.
  output: "export",
  trailingSlash: true,
  // Image optimisation is not available in static export mode.
  images: { unoptimized: true },
};

export default nextConfig;
