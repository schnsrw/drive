// Astro 5 config — static output, MDX, sitemap.
// Spec: docs/research/07-marketing-site.md.
import { defineConfig } from "astro/config";
import mdx from "@astrojs/mdx";
import sitemap from "@astrojs/sitemap";

// Production site URL. Currently `drive.schnsrw.live` (the GitHub Pages
// target — see marketing/public/CNAME). Flip to https://casualoffice.org
// or https://schnsrw.live when DNS lands (§15.13). For GitHub Pages
// org-site deploys (https://<org>.github.io/drive/) keep `base` too.
// CI overrides via repo Variables: `MARKETING_SITE_URL`, `MARKETING_SITE_BASE`.
const SITE = process.env.ASTRO_SITE || "https://drive.schnsrw.live";
const BASE = process.env.ASTRO_BASE || undefined;

export default defineConfig({
  site: SITE,
  base: BASE,
  output: "static",
  trailingSlash: "ignore",
  integrations: [
    mdx(),
    sitemap({
      // /demo is the running SPA — exclude from sitemap so it doesn't get
      // indexed (robots.txt + per-page noindex meta also enforce this).
      filter: (page) => !page.includes("/demo"),
    }),
  ],
  build: {
    inlineStylesheets: "auto",
    assets: "_assets",
  },
  compressHTML: true,
  vite: {
    build: {
      // Drop console in production. Astro inherits this through vite.
      cssCodeSplit: true,
    },
  },
  image: {
    // Sharp service ships AVIF + WebP + responsive srcsets.
    service: { entrypoint: "astro/assets/services/sharp" },
  },
});
