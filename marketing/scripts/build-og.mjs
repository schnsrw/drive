/**
 * Compose the Open Graph / Twitter card. 1200×630, brand-matched.
 * Spec: docs/research/07-marketing-site.md §"SEO checklist".
 *
 * Inputs: nothing (literal SVG below). Output: marketing/public/og/default.png.
 *
 * Run via `pnpm og` from marketing/ — no server, no browser. Sharp rasterises
 * the inline SVG; we ship the PNG so social cards stay snappy + cacheable.
 */
import sharp from "sharp";
import { mkdir, writeFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const OUT = resolve(HERE, "../public/og/default.png");
const W = 1200;
const H = 630;

// Brand colours match marketing/src/styles/tokens.css (light theme).
const PAPER = "#fbf9f4";
const PAPER_2 = "#f5f1e8";
const INK = "#1a1a1e";
const MUTED = "#6b6b73";
const ACCENT = "#c8a45c";
const ACCENT_SOFT = "rgba(200,164,92,0.14)";
const LINE = "rgba(26,26,30,0.08)";

const svg = Buffer.from(`<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" width="${W}" height="${H}" viewBox="0 0 ${W} ${H}">
  <defs>
    <radialGradient id="bg" cx="22%" cy="0%" r="80%">
      <stop offset="0%"  stop-color="${ACCENT_SOFT}"/>
      <stop offset="60%" stop-color="${PAPER}"/>
      <stop offset="100%" stop-color="${PAPER_2}"/>
    </radialGradient>
    <clipPath id="markClip"><rect width="60" height="60" rx="14"/></clipPath>
  </defs>

  <rect width="${W}" height="${H}" fill="url(#bg)"/>

  <!-- Card outline / inner bleed -->
  <rect x="20" y="20" width="${W - 40}" height="${H - 40}" rx="28"
        fill="none" stroke="${LINE}" stroke-width="1"/>

  <!-- Brand row: cloud mark + wordmark -->
  <g transform="translate(80,82)">
    <g clip-path="url(#markClip)">
      <rect width="60" height="60" fill="${INK}"/>
      <g fill="${PAPER}">
        <circle cx="19" cy="35" r="8"/>
        <circle cx="41" cy="35" r="8"/>
        <circle cx="30" cy="23" r="12"/>
        <rect x="19" y="35" width="22" height="8"/>
      </g>
    </g>
    <text x="80" y="24"
          font-family="Inter, system-ui, sans-serif"
          font-size="20" font-weight="600"
          letter-spacing="0.05em" fill="${INK}">
      CASUAL DRIVE
    </text>
    <text x="80" y="48"
          font-family="Inter, system-ui, sans-serif"
          font-size="14" font-weight="500" fill="${MUTED}">
      Open-source · self-hosted · Rust + React
    </text>
  </g>

  <!-- Headline -->
  <text x="80" y="280"
        font-family="Inter, system-ui, sans-serif"
        font-size="76" font-weight="700"
        letter-spacing="-0.03em" fill="${INK}">
    Your files. Your editors.
  </text>
  <text x="80" y="368"
        font-family="Inter, system-ui, sans-serif"
        font-size="76" font-weight="700"
        letter-spacing="-0.03em" fill="${ACCENT}">
    Your server.
  </text>

  <!-- Lede -->
  <text x="80" y="432"
        font-family="Inter, system-ui, sans-serif"
        font-size="26" font-weight="400" fill="${MUTED}">
    Self-hosted Drive that opens .xlsx and .docx in the browser.
  </text>

  <!-- Bullet row -->
  <g transform="translate(80,500)" font-family="Inter, system-ui, sans-serif"
     font-size="20" font-weight="500" fill="${INK}">
    <g>
      <circle cx="6" cy="-6" r="3" fill="${ACCENT}"/>
      <text x="20" y="0">MIT licensed</text>
    </g>
    <g transform="translate(200,0)">
      <circle cx="6" cy="-6" r="3" fill="${ACCENT}"/>
      <text x="20" y="0">Single binary</text>
    </g>
    <g transform="translate(420,0)">
      <circle cx="6" cy="-6" r="3" fill="${ACCENT}"/>
      <text x="20" y="0">S3 / MinIO / fs</text>
    </g>
    <g transform="translate(660,0)">
      <circle cx="6" cy="-6" r="3" fill="${ACCENT}"/>
      <text x="20" y="0">No tracking</text>
    </g>
  </g>

  <!-- Footer hostname -->
  <text x="${W - 80}" y="${H - 50}"
        text-anchor="end"
        font-family="Inter, system-ui, sans-serif"
        font-size="18" font-weight="500" fill="${MUTED}">
    schnsrw.live
  </text>
</svg>
`);

await mkdir(dirname(OUT), { recursive: true });
const png = await sharp(svg).png({ compressionLevel: 9 }).toBuffer();
await writeFile(OUT, png);

const size = (png.length / 1024).toFixed(1);
console.log(`→ wrote ${OUT} (${size} KB, ${W}×${H})`);
