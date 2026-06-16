#!/usr/bin/env node
/**
 * Copies the SDK iframe-embed runtime (embed.html + embed-runtime.*)
 * from each editor package's dist into Drive's public/embed/ tree so
 * the SPA can serve them same-origin. The `<CasualSheetsIframe>` and
 * `<CasualEditorIframe>` components default `embedBasePath` to
 * `/embed/sheets` and `/embed/docs` — these paths exist after this
 * script runs and after Vite copies public/ to dist/.
 *
 * Runs at prebuild time (see package.json's `prebuild` script). The
 * resulting files are NOT committed (see .gitignore); they regenerate
 * whenever the SDK deps update.
 */
import { cpSync, mkdirSync, existsSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { createRequire } from "node:module";

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, "..");
const require_ = createRequire(import.meta.url);

const PACKAGES = [
  // [npm name, public/embed/<subdir>, exports-key we use to anchor to the embed/ dir]
  ["@schnsrw/casual-sheets", "sheets", "embed/embed.html"],
  ["@schnsrw/docx-js-editor", "docs", "embed/embed.html"],
];

let failed = false;

for (const [pkg, subdir, anchor] of PACKAGES) {
  try {
    // Both packages restrict the `exports` field — `package.json` isn't
    // exported. Instead, resolve a known export (embed.html) and walk
    // back to its containing directory.
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

    // doc 1.1.4 ships dist/embed/embed-runtime.css that only contains
    // ProseMirror base styles — the .ep-root tailwind-compiled editor
    // CSS lives at dist/styles.css and isn't bundled into the embed
    // runtime. Without it, every toolbar button stacks vertically with
    // no layout and the editor canvas paints unstyled. Concatenate the
    // editor styles into the embed CSS so the <link> in embed.html
    // serves the full stylesheet.
    if (pkg === "@schnsrw/docx-js-editor") {
      const editorStylesPath = resolve(dirname(srcEmbedDir), "styles.css");
      const embedCssPath = resolve(dstDir, "embed-runtime.css");
      if (existsSync(editorStylesPath)) {
        const base = existsSync(embedCssPath) ? readFileSync(embedCssPath, "utf8") : "";
        const editor = readFileSync(editorStylesPath, "utf8");
        writeFileSync(embedCssPath, `${base}\n${editor}`);
      }
    }

    // Patch the embed.html in place for three upstream packaging bugs:
    //   - sheet 0.5.0 references `embed-runtime.css` but the SDK doesn't
    //     ship one. Strip the stylesheet link so the browser doesn't
    //     log a 404 — the embed-runtime.js inlines its own styles.
    //   - doc 1.1.0 imports `./embed-runtime.js` but the dist actually
    //     ships `.mjs`. Rewrite the import to point at the real file.
    //   - sheet 0.5.x bundles `from "crypto"` as a bare specifier; the
    //     browser can't resolve bare specifiers without an importmap.
    //     Inject one that shims "crypto" to a thin wrapper around
    //     globalThis.crypto (WebCrypto exists in every modern browser).
    // All three are upstream issues; this keeps drive's runtime quiet
    // until the SDKs ship a corrected build.
    const htmlPath = resolve(dstDir, "embed.html");
    if (existsSync(htmlPath)) {
      const raw = readFileSync(htmlPath, "utf8");
      let patched = raw;

      // Force the iframe to render light theme — Drive itself is light
      // only today, and the SDK's `data-theme="auto"` default makes the
      // iframe diverge whenever the user's OS prefers dark. We can't
      // ship dark mode parity until Drive has a theme toggle and a
      // host → iframe `command.set.theme` hookup. Until then, pin the
      // iframe to light so the chrome matches the host page.
      //
      // Implementation: an inline <script> set to fire BEFORE the SDK's
      // own module-script. It sets data-theme="light" on initial parse
      // AND installs a MutationObserver that resets the attribute if
      // the SDK toggles it after mount. Both surfaces (sheet + doc).
      const themeLock =
        `<script>` +
        `document.documentElement.setAttribute('data-theme','light');` +
        `new MutationObserver(function(){` +
        `if(document.documentElement.getAttribute('data-theme')!=='light'){` +
        `document.documentElement.setAttribute('data-theme','light');` +
        `}` +
        `}).observe(document.documentElement,{attributes:true,attributeFilter:['data-theme']});` +
        `</script>`;
      if (!patched.includes("data-theme','light'")) {
        // Inject right after </style> so it runs after the inline CSS
        // is in place but before the module script imports the SDK.
        if (patched.includes("</style>")) {
          patched = patched.replace("</style>", "</style>\n    " + themeLock);
        } else if (patched.includes('<script type="module"')) {
          patched = patched.replace(
            /<script type="module"/,
            themeLock + '\n    <script type="module"',
          );
        }
      }

      const cssFile = resolve(dstDir, "embed-runtime.css");
      if (!existsSync(cssFile)) {
        patched = patched.replace(
          /\s*<link rel="stylesheet" href="\.\/embed-runtime\.css" \/>\s*/g,
          "\n    ",
        );
      }
      const jsFile = resolve(dstDir, "embed-runtime.js");
      const mjsFile = resolve(dstDir, "embed-runtime.mjs");
      if (!existsSync(jsFile) && existsSync(mjsFile)) {
        patched = patched.replace(/embed-runtime\.js/g, "embed-runtime.mjs");
      }

      // Inject the "crypto" bare-specifier shim only when the bundled
      // runtime actually references it. Re-exports node-crypto's
      // commonly-used surface from WebCrypto.
      const runtimeFile = existsSync(jsFile) ? jsFile : existsSync(mjsFile) ? mjsFile : null;
      const needsCryptoShim = runtimeFile
        ? /from\s*["']crypto["']|import\s*["']crypto["']/.test(readFileSync(runtimeFile, "utf8"))
        : false;
      if (needsCryptoShim && !patched.includes('id="cd-embed-importmap"')) {
        // Pure-JS SHA-256 — the bundled doc runtime calls
        // `createHash('sha256').update(e).digest()` synchronously, and
        // WebCrypto's subtle.digest is async, so a polyfill is the only
        // option here. ~60 lines of well-known FIPS 180-4 pseudocode.
        // Returns a Uint8Array matching node's Buffer surface that the
        // runtime reads byte-wise.
        const sha256 =
          "const K=new Uint32Array([0x428a2f98,0x71374491,0xb5c0fbcf,0xe9b5dba5,0x3956c25b,0x59f111f1,0x923f82a4,0xab1c5ed5,0xd807aa98,0x12835b01,0x243185be,0x550c7dc3,0x72be5d74,0x80deb1fe,0x9bdc06a7,0xc19bf174,0xe49b69c1,0xefbe4786,0x0fc19dc6,0x240ca1cc,0x2de92c6f,0x4a7484aa,0x5cb0a9dc,0x76f988da,0x983e5152,0xa831c66d,0xb00327c8,0xbf597fc7,0xc6e00bf3,0xd5a79147,0x06ca6351,0x14292967,0x27b70a85,0x2e1b2138,0x4d2c6dfc,0x53380d13,0x650a7354,0x766a0abb,0x81c2c92e,0x92722c85,0xa2bfe8a1,0xa81a664b,0xc24b8b70,0xc76c51a3,0xd192e819,0xd6990624,0xf40e3585,0x106aa070,0x19a4c116,0x1e376c08,0x2748774c,0x34b0bcb5,0x391c0cb3,0x4ed8aa4a,0x5b9cca4f,0x682e6ff3,0x748f82ee,0x78a5636f,0x84c87814,0x8cc70208,0x90befffa,0xa4506ceb,0xbef9a3f7,0xc67178f2]);" +
          "function rr(x,n){return (x>>>n)|(x<<(32-n));}" +
          "function sha256(bytes){const len=bytes.length;const bitLen=len*8;const padLen=((len+9+63)&~63);const msg=new Uint8Array(padLen);msg.set(bytes);msg[len]=0x80;const dv=new DataView(msg.buffer);dv.setUint32(padLen-4,bitLen>>>0);dv.setUint32(padLen-8,Math.floor(bitLen/0x100000000));let H=new Uint32Array([0x6a09e667,0xbb67ae85,0x3c6ef372,0xa54ff53a,0x510e527f,0x9b05688c,0x1f83d9ab,0x5be0cd19]);const w=new Uint32Array(64);for(let i=0;i<padLen;i+=64){for(let t=0;t<16;t++)w[t]=dv.getUint32(i+t*4);for(let t=16;t<64;t++){const s0=rr(w[t-15],7)^rr(w[t-15],18)^(w[t-15]>>>3);const s1=rr(w[t-2],17)^rr(w[t-2],19)^(w[t-2]>>>10);w[t]=(w[t-16]+s0+w[t-7]+s1)|0;}let [a,b,c,d,e,f,g,h]=H;for(let t=0;t<64;t++){const S1=rr(e,6)^rr(e,11)^rr(e,25);const ch=(e&f)^(~e&g);const T1=(h+S1+ch+K[t]+w[t])|0;const S0=rr(a,2)^rr(a,13)^rr(a,22);const mj=(a&b)^(a&c)^(b&c);const T2=(S0+mj)|0;h=g;g=f;f=e;e=(d+T1)|0;d=c;c=b;b=a;a=(T1+T2)|0;}H[0]=(H[0]+a)|0;H[1]=(H[1]+b)|0;H[2]=(H[2]+c)|0;H[3]=(H[3]+d)|0;H[4]=(H[4]+e)|0;H[5]=(H[5]+f)|0;H[6]=(H[6]+g)|0;H[7]=(H[7]+h)|0;}const out=new Uint8Array(32);const odv=new DataView(out.buffer);for(let i=0;i<8;i++)odv.setUint32(i*4,H[i]);return out;}" +
          "function toBytes(input){if(input instanceof Uint8Array)return input;if(typeof input==='string')return new TextEncoder().encode(input);if(ArrayBuffer.isView(input))return new Uint8Array(input.buffer,input.byteOffset,input.byteLength);if(input instanceof ArrayBuffer)return new Uint8Array(input);throw new TypeError('unsupported input to createHash');}" +
          "function hexOf(u8){let s='';for(let i=0;i<u8.length;i++){const b=u8[i];s+=(b<16?'0':'')+b.toString(16);}return s;}" +
          "function b64Of(u8){let s='';for(let i=0;i<u8.length;i++)s+=String.fromCharCode(u8[i]);return btoa(s);}";
        const shim =
          sha256 +
          "export const webcrypto = globalThis.crypto;" +
          "export const randomUUID = () => globalThis.crypto.randomUUID();" +
          "export const getRandomValues = (a) => globalThis.crypto.getRandomValues(a);" +
          "export function createHash(alg){if(String(alg).toLowerCase()!=='sha256')throw new Error('crypto shim: only sha256 is implemented (asked for '+alg+')');" +
          "const buf=[];return {update(d){buf.push(toBytes(d));return this;}," +
          "digest(enc){let total=0;for(const b of buf)total+=b.length;const all=new Uint8Array(total);let off=0;for(const b of buf){all.set(b,off);off+=b.length;}const out=sha256(all);if(enc==='hex')return hexOf(out);if(enc==='base64')return b64Of(out);return out;}};}" +
          "export default {webcrypto:globalThis.crypto,createHash,randomUUID,getRandomValues:(a)=>globalThis.crypto.getRandomValues(a)};";
        const importmap =
          '<script id="cd-embed-importmap" type="importmap">' +
          JSON.stringify({
            imports: { crypto: "data:text/javascript;base64," + Buffer.from(shim).toString("base64") },
          }) +
          "</script>";
        // Inline globals shim — `process`, `global`, and a minimal
        // `Buffer` (Uint8Array-backed with the toString/concat/from/alloc
        // surface the SDK bundles read). Runs BEFORE the importmap so
        // any module that touches these globals at top-level (the SDK
        // does) finds them defined.
        const globalsShim =
          "<script>" +
          "(function(){" +
          "if(typeof globalThis.global==='undefined')globalThis.global=globalThis;" +
          "if(typeof globalThis.process==='undefined')globalThis.process={env:{NODE_ENV:'production'},argv:[],browser:true,version:'',versions:{node:''},nextTick:function(cb){var a=Array.prototype.slice.call(arguments,1);queueMicrotask(function(){cb.apply(null,a);});},platform:'browser',on:function(){},off:function(){},emit:function(){},stdout:{write:function(){}},stderr:{write:function(){}}};" +
          "if(typeof globalThis.Buffer==='undefined'){" +
          "function B(input,encoding){" +
          "if(typeof input==='number'){var a=new Uint8Array(input);return Object.setPrototypeOf(a,B.prototype);}" +
          "if(typeof input==='string'){var enc=encoding||'utf8';var out;" +
          "if(enc==='hex'){out=new Uint8Array(input.length/2);for(var i=0;i<out.length;i++)out[i]=parseInt(input.substr(i*2,2),16);}" +
          "else if(enc==='base64'){var bin=atob(input);out=new Uint8Array(bin.length);for(var j=0;j<bin.length;j++)out[j]=bin.charCodeAt(j);}" +
          "else{out=new TextEncoder().encode(input);}" +
          "return Object.setPrototypeOf(out,B.prototype);}" +
          "if(input instanceof Uint8Array)return Object.setPrototypeOf(new Uint8Array(input.buffer,input.byteOffset,input.byteLength),B.prototype);" +
          "if(input instanceof ArrayBuffer)return Object.setPrototypeOf(new Uint8Array(input),B.prototype);" +
          "if(Array.isArray(input))return Object.setPrototypeOf(new Uint8Array(input),B.prototype);" +
          "throw new TypeError('Buffer shim: unsupported input');" +
          "}" +
          "B.prototype=Object.create(Uint8Array.prototype);" +
          "B.prototype.constructor=B;" +
          "B.prototype.toString=function(enc){enc=enc||'utf8';" +
          "if(enc==='hex'){var s='';for(var i=0;i<this.length;i++){var b=this[i];s+=(b<16?'0':'')+b.toString(16);}return s;}" +
          "if(enc==='base64'){var bin='';for(var j=0;j<this.length;j++)bin+=String.fromCharCode(this[j]);return btoa(bin);}" +
          "return new TextDecoder('utf-8').decode(this);};" +
          "B.from=function(a,b){return B(a,b);};" +
          "B.alloc=function(n,fill){var x=B(n);if(fill!==undefined){for(var i=0;i<n;i++)x[i]=typeof fill==='string'?fill.charCodeAt(0):fill;}return x;};" +
          "B.isBuffer=function(x){return x instanceof B;};" +
          "B.concat=function(list,total){if(total===undefined){total=0;for(var i=0;i<list.length;i++)total+=list[i].length;}var out=B(total);var off=0;for(var j=0;j<list.length;j++){out.set(list[j],off);off+=list[j].length;}return out;};" +
          "B.byteLength=function(s,enc){return new TextEncoder().encode(typeof s==='string'?s:'').length;};" +
          "globalThis.Buffer=B;" +
          "}" +
          "})();" +
          "</script>";
        const injection = `${globalsShim}\n    ${importmap}`;
        // Importmaps must appear before the first module script tag.
        if (/<head[^>]*>/.test(patched)) {
          patched = patched.replace(/(<head[^>]*>)/, `$1\n    ${injection}`);
        } else {
          patched = patched.replace(/(<script\s+type="module")/, `${injection}\n    $1`);
        }
      }

      // NB: the previous "cd-iframe-baseline" injection (IBM Plex Sans
      // typography + hardcoded --doc-* design tokens) is intentionally
      // removed. It was justified pre-1.1.5 when the SDK didn't bundle
      // its dist/styles.css into embed-runtime.css; the baseline kept
      // the iframe from rendering with empty var(...) substitutions.
      // Since 1.1.5 the SDK ships the full tailwind layer (with its
      // own canonical token values) inside embed-runtime.css, AND the
      // copy-embed step above concatenates styles.css for older SDKs.
      // Keeping the baseline now LEAKS Drive's typography into the
      // iframe — IBM Plex Sans overrides Univer's Calibri/Arial and
      // breaks the doc editor's own font stack. Trust the SDK's CSS.

      if (patched !== raw) {
        writeFileSync(htmlPath, patched);
        console.log(`[copy-embed] ${pkg}: patched embed.html`);
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
