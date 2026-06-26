/**
 * Next.js configuration for alpha-compass.
 *
 * The frontend is a static-exported SPA loaded by the Tauri 2 webview.
 * - `output: 'export'` disables SSR and emits a static bundle to `out/`.
 * - `images.unoptimized` is required because there is no Next.js image server
 *   when running inside Tauri.
 * - `assetPrefix` is left empty so assets resolve relative to the Tauri asset
 *   protocol root.
 */

/** @type {import('next').NextConfig} */
const nextConfig = {
  output: 'export',
  // Tauri serves the bundle from a custom protocol; keep links relative.
  trailingSlash: true,
  images: {
    unoptimized: true,
  },
  // Helpful while iterating against the Rust IPC surface.
  reactStrictMode: true,
};

module.exports = nextConfig;
