/**
 * `/sitemap.xml` alias. Astro's @astrojs/sitemap emits `sitemap-index.xml`
 * + `sitemap-N.xml`, which is the spec-correct shape — but humans (and a
 * few legacy crawlers) hit `/sitemap.xml` first by convention. This route
 * mirrors the index so both URLs resolve.
 *
 * `prerender = true` keeps it a static file at build time.
 */
import type { APIRoute } from "astro";

export const prerender = true;

export const GET: APIRoute = ({ site }) => {
  const origin = (site?.toString() ?? "https://schnsrw.live").replace(/\/$/, "");
  // Mirror the @astrojs/sitemap index format byte-for-byte so crawlers
  // see identical content at either URL.
  const body =
    `<?xml version="1.0" encoding="UTF-8"?>` +
    `<sitemapindex xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">` +
    `<sitemap><loc>${origin}/sitemap-0.xml</loc></sitemap>` +
    `</sitemapindex>`;
  return new Response(body, {
    headers: { "content-type": "application/xml; charset=utf-8" },
  });
};
