/**
 * Dynamic robots.txt — substitutes the configured ASTRO_SITE so deploys
 * to other domains (custom DNS, preview environments) emit the right
 * Sitemap URL. `prerender = true` keeps it a static file at build time.
 *
 * /demo + /demo-app/ are disallowed by name — the demo is the running app,
 * not crawlable content, and indexing it would pollute search with
 * stateful pages.
 */
import type { APIRoute } from "astro";

export const prerender = true;

export const GET: APIRoute = ({ site }) => {
  const origin = (site?.toString() ?? "https://schnsrw.live").replace(/\/$/, "");
  const body = `# robots.txt — managed by marketing/src/pages/robots.txt.ts
User-agent: *
Allow: /
Disallow: /demo
Disallow: /demo-app/

Sitemap: ${origin}/sitemap-index.xml
`;
  return new Response(body, {
    headers: { "content-type": "text/plain; charset=utf-8" },
  });
};
