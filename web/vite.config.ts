import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { defineConfig, loadEnv } from "vite";

// In dev, proxy API + WOPI + health endpoints through to the backend bound at
// DRIVE_DEV_BACKEND (default http://127.0.0.1:18090). The SPA itself serves
// on its own port — Vite's dev server.

export default defineConfig(({ mode }) => {
  const env = loadEnv(mode, process.cwd(), "");
  const backend = env.DRIVE_DEV_BACKEND ?? "http://127.0.0.1:18090";
  // Asset root override. Real-Drive builds embed into the binary at "/";
  // the marketing site mounts the demo at "/demo-app/" and needs hashed
  // asset URLs scoped under that prefix. CI sets VITE_BASE accordingly.
  const base = env.VITE_BASE ?? "/";

  return {
    base,
    plugins: [
      react(),
      tailwindcss(),
      {
        // @schnsrw/docx-js-editor@1.0.0's dist contains a runtime
        // `new Worker(new URL("./format-converter.worker.ts", import.meta.url))`
        // but the worker source isn't bundled — Vite's worker-import-meta-url
        // plugin throws at build time trying to resolve it. Drive never
        // triggers the converter (only .docx through DriveFileSource), so
        // rewrite the worker construction to a no-op stub before Vite sees
        // it. Long-term fix is republishing the editor SDK with the worker
        // properly bundled; tracked separately.
        name: "casual-drive-sdk-worker-shim",
        enforce: "pre",
        transform(code, id) {
          if (
            !id.includes("@schnsrw/docx-js-editor") ||
            !code.includes("format-converter.worker.ts")
          ) {
            return null;
          }
          return code.replace(
            /new Worker\(new URL\(["']\.\/format-converter\.worker\.ts["'],import\.meta\.url\)\s*,\s*\{[^}]*\}\)/g,
            "({" +
              "postMessage(){}," +
              "addEventListener(){}," +
              "removeEventListener(){}," +
              "terminate(){}," +
              "onmessage:null,onerror:null,onmessageerror:null" +
              "})",
          );
        },
      },
    ],
    server: {
      port: 5173,
      strictPort: true,
      proxy: {
        "/api": { target: backend, changeOrigin: true },
        "/healthz": { target: backend, changeOrigin: true },
        "/wopi": { target: backend, changeOrigin: true },
      },
    },
    build: {
      outDir: "dist",
      sourcemap: false,
      assetsDir: "assets",
      // The editor SDK + Univer + ProseMirror combined push the index
      // chunk past 2 MB. Split them into dedicated vendor chunks so the
      // shell stays small and lazy-loaded surfaces (the Preview modal's
      // doc / sheet stages) pull the heavy bundles only when actually
      // opened.
      rollupOptions: {
        output: {
          manualChunks(id) {
            if (!id.includes("node_modules")) return undefined;
            if (id.includes("@univerjs/")) return "vendor-univer";
            if (id.includes("@schnsrw/casual-sheets")) return "vendor-univer";
            if (id.includes("@schnsrw/docx-js-editor")) return "vendor-docx-editor";
            if (id.includes("prosemirror-")) return "vendor-docx-editor";
            if (id.includes("yjs") || id.includes("y-prosemirror") || id.includes("y-websocket")) {
              return "vendor-collab";
            }
            if (id.includes("react") && !id.includes("react-")) return "vendor-react";
            return undefined;
          },
        },
      },
    },
  };
});
