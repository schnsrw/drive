#!/usr/bin/env node
/**
 * Copies the SDK iframe-embed runtimes from each editor package's dist
 * into Drive's public/embed/ tree so the SPA can serve them same-origin.
 * Drive is a HOST that embeds each editor in a sandboxed `<iframe>`; the
 * iframe wrappers point `embedBasePath` at `${BASE_URL}embed/sheets`
 * (`<SheetEmbed>`) and `${BASE_URL}embed/docs` (`<CasualEditorIframe>`) —
 * those paths exist after this script runs and after Vite copies public/
 * to dist/.
 *
 * Runs at the front of `dev` + `build` (see package.json). The copied
 * files are NOT committed (.gitignore); they regenerate whenever the SDK
 * deps update.
 *
 * Per-package handling:
 *   - @casualoffice/sheets (>=0.11): the published embed is clean —
 *     a self-contained `embed-runtime.js` (Univer CSS inlined at runtime),
 *     an `embed.html` with no external <link> and no bare-specifier
 *     imports, plus the parser/exporter workers. Pure copy, no patching.
 *     (The crypto/Buffer/theme-lock shims older Drive builds carried were
 *     workarounds for pre-0.11 SDK packaging bugs that are now fixed.)
 *   - @schnsrw/docx-js-editor: its `embed.html` imports `./embed-runtime.js`
 *     while the dist file is `.mjs`, so rewrite that import. Its
 *     `embed-runtime.css` is shipped + linked correctly; concatenate the
 *     editor `styles.css` into it as a belt-and-suspenders for older builds
 *     whose embed CSS lacked the `.ep-root` editor layer.
 */
import { cpSync, mkdirSync, existsSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { createRequire } from "node:module";

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, "..");
const require_ = createRequire(import.meta.url);

const PACKAGES = [
  // [npm name, public/embed/<subdir>, exports-key anchoring the embed dir]
  ["@casualoffice/sheets", "sheets", "embed/embed.html"],
  ["@schnsrw/docx-js-editor", "docs", "embed/embed.html"],
];

let failed = false;

for (const [pkg, subdir, anchor] of PACKAGES) {
  try {
    // Both packages restrict `exports` (package.json isn't exported), so
    // resolve a known export (embed.html) and walk back to its directory.
    const anchorPath = require_.resolve(`${pkg}/${anchor}`);
    const srcEmbedDir = dirname(anchorPath);
    if (!existsSync(srcEmbedDir)) {
      console.error(`[copy-embed] ${pkg}: ${srcEmbedDir} doesn't exist`);
      failed = true;
      continue;
    }

    const dstDir = resolve(root, "public", "embed", subdir);
    rmSync(dstDir, { recursive: true, force: true });
    mkdirSync(dstDir, { recursive: true });
    cpSync(srcEmbedDir, dstDir, { recursive: true });

    // docs-only fix-ups. The sheets embed needs none — it's copied as-is.
    if (pkg === "@schnsrw/docx-js-editor") {
      // Concatenate the editor's tailwind-compiled styles into the embed
      // CSS so the toolbar + canvas paint with full layout even on builds
      // whose embed-runtime.css shipped only ProseMirror base styles.
      const editorStylesPath = resolve(dirname(srcEmbedDir), "styles.css");
      const embedCssPath = resolve(dstDir, "embed-runtime.css");
      if (existsSync(editorStylesPath)) {
        const base = existsSync(embedCssPath) ? readFileSync(embedCssPath, "utf8") : "";
        const editor = readFileSync(editorStylesPath, "utf8");
        writeFileSync(embedCssPath, `${base}\n${editor}`);
      }

      // The embed.html imports `./embed-runtime.js` but the dist ships
      // `.mjs`; rewrite so the browser resolves the real file.
      const htmlPath = resolve(dstDir, "embed.html");
      const jsFile = resolve(dstDir, "embed-runtime.js");
      const mjsFile = resolve(dstDir, "embed-runtime.mjs");
      if (existsSync(htmlPath) && !existsSync(jsFile) && existsSync(mjsFile)) {
        const raw = readFileSync(htmlPath, "utf8");
        const patched = raw.replace(/embed-runtime\.js/g, "embed-runtime.mjs");
        if (patched !== raw) {
          writeFileSync(htmlPath, patched);
          console.log(`[copy-embed] ${pkg}: rewrote embed-runtime.js → .mjs in embed.html`);
        }
      }
    }

    const copied = readdirSync(dstDir);
    console.log(`[copy-embed] ${pkg} → public/embed/${subdir}/  (${copied.length} files)`);
    for (const f of copied) console.log(`[copy-embed]   ${f}`);
  } catch (err) {
    console.error(`[copy-embed] ${pkg}: ${err instanceof Error ? err.message : err}`);
    failed = true;
  }
}

if (failed) {
  console.error("[copy-embed] one or more packages failed; aborting build");
  process.exit(1);
}
