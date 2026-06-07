/**
 * Marketing screenshot harness. Drives the demo-mode SPA via Playwright,
 * captures every flagship surface in light + dark, desktop + mobile,
 * and writes PNGs into marketing/public/screenshots/.
 *
 * Spec: docs/ux/14-marketing-surface.md.
 *
 * Run from repo root:
 *   ( cd web && VITE_DEMO_MODE=1 pnpm build && pnpm preview --port 4173 --strictPort --host 127.0.0.1 ) &
 *   node web/tests/e2e/marketing-screenshots.mjs
 *
 * Or `pnpm screenshots` from web/.
 *
 * The harness wipes localStorage between runs so we always start from
 * the demo's seeded state. Theme is toggled by setting the
 * `cd-theme-v1` localStorage key before navigation.
 */
import { chromium } from "@playwright/test";
import { existsSync, mkdirSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const OUT_DIR = resolve(HERE, "../../../marketing/public/screenshots");
const URL_BASE = process.env.URL ?? "http://127.0.0.1:4173/";

if (!existsSync(OUT_DIR)) mkdirSync(OUT_DIR, { recursive: true });

const VIEWPORTS = {
  desktop: { width: 1440, height: 900, scale: 2 },
  // iPhone 14 Pro-ish — typical mobile-first target.
  mobile: { width: 393, height: 852, scale: 2 },
};

// Dark palette landed in §2.11 — both themes now flip real CSS variables.
// The marketing gallery uses the light shots by default; dark variants
// are emitted alongside as `<id>-dark.png` so the gallery can light them
// up when a dark-mode preference is added.
const THEMES = ["light", "dark"];

/**
 * Each shot: an id (becomes the filename stem), a viewport key, a theme,
 * and an async setup function that brings the SPA to the desired surface.
 * The harness writes `<id>-<theme>.png` for every (id, theme).
 *
 * Mobile-specific shots set viewport: "mobile" — they only render at the
 * mobile breakpoint.
 */
const SHOTS = [
  {
    id: "files-list",
    viewport: "desktop",
    setup: async (page) => {
      await signInDemo(page);
      await ensureWorkspace(page, "Casual Demo");
      await page.waitForTimeout(400);
    },
  },
  {
    id: "files-mobile",
    viewport: "mobile",
    setup: async (page) => {
      await signInDemo(page);
      await page.waitForTimeout(400);
    },
  },
  {
    id: "share-dialog",
    viewport: "desktop",
    setup: async (page) => {
      await signInDemo(page);
      await page.waitForTimeout(300);
      // Right-click the first file row to open the kebab menu.
      const row = page.locator('[role="row"]').nth(1);
      if (await row.count()) {
        await row.click({ button: "right" });
        await page.waitForTimeout(200);
      }
      const shareItem = page.getByText(/^Share/).first();
      if (await shareItem.count()) {
        await shareItem.click();
        await page.waitForTimeout(400);
      }
    },
  },
  {
    id: "settings-storage",
    viewport: "desktop",
    setup: async (page) => {
      await signInDemo(page);
      await page.getByRole("button", { name: /Settings/ }).first().click();
      await page.waitForTimeout(200);
      const storageNav = page.getByRole("button", { name: /Storage/ }).first();
      if (await storageNav.count()) await storageNav.click();
      await page.waitForTimeout(400);
    },
  },
  {
    id: "admin-users",
    viewport: "desktop",
    setup: async (page) => {
      await signInDemo(page);
      await page.getByRole("button", { name: /Admin/ }).first().click();
      await page.waitForTimeout(400);
    },
  },
  {
    id: "activity",
    viewport: "desktop",
    setup: async (page) => {
      await signInDemo(page);
      await page.getByRole("button", { name: /Activity/ }).first().click();
      await page.waitForTimeout(400);
    },
  },
  {
    id: "notes",
    viewport: "desktop",
    setup: async (page) => {
      await signInDemo(page);
      await page.getByRole("button", { name: /^Notes/ }).first().click();
      await page.waitForTimeout(200);
      // Create a note + write some body so the editor shows something useful.
      const newBtn = page.getByRole("button", { name: /New page/ }).first();
      if (await newBtn.count()) await newBtn.click();
      await page.waitForTimeout(200);
      const title = page.getByPlaceholder(/Title…/);
      if (await title.count()) await title.fill("Sprint planning");
      const body = page.getByPlaceholder(/Start writing in markdown/);
      if (await body.count()) {
        await body.fill(
          "# Sprint planning\n\nFor the **June kick-off**:\n\n" +
            "- Migration timeline locked.\n" +
            "- Owners: Alex, Sam, Priya.\n" +
            "- Linked from [[Onboarding]] and [[Q3 roadmap]].\n\n" +
            "Decisions land in [[Decisions]].\n",
        );
      }
      await page.waitForTimeout(600); // let preview + debounce settle
    },
  },
];

const browser = await chromium.launch();

for (const shot of SHOTS) {
  const vp = VIEWPORTS[shot.viewport];
  for (const theme of THEMES) {
    const ctx = await browser.newContext({
      viewport: { width: vp.width, height: vp.height },
      deviceScaleFactor: vp.scale,
      reducedMotion: "reduce", // avoid mid-animation captures
      // Drive the `prefers-color-scheme` media query directly. tokens.css
      // mirrors the dark palette under both `:root[data-theme="dark"]`
      // AND `@media (prefers-color-scheme: dark) :root[data-theme="system"]`,
      // so combining this with `theme="system"` in localStorage flips the
      // whole UI without racing against the React ThemeToggle effect.
      colorScheme: theme,
    });
    // Also seed localStorage so the ThemeToggle's first render agrees.
    // `system` is the cleanest setting — it defers to colorScheme above.
    await ctx.addInitScript(() => {
      try {
        window.localStorage.setItem("theme", "system");
      } catch {
        /* ignored */
      }
    });

    const page = await ctx.newPage();
    try {
      await shot.setup(page);
    } catch (err) {
      console.error(`[${shot.id}/${theme}] setup failed:`, err.message);
      await ctx.close();
      continue;
    }

    // Light is the canonical/primary asset → `<id>.png`. Dark variant
    // ships alongside as `<id>-dark.png` so a media-query / theme-toggle
    // on the marketing side can swap without filename churn.
    const out =
      theme === "light"
        ? `${OUT_DIR}/${shot.id}.png`
        : `${OUT_DIR}/${shot.id}-${theme}.png`;
    await page.screenshot({ path: out, fullPage: false });
    console.log(`→ ${out}`);
    await ctx.close();
  }
}

await browser.close();

// ── helpers ──────────────────────────────────────────────────────────

async function signInDemo(page) {
  await page.goto(URL_BASE);
  // Demo build pre-fills credentials.
  await page.waitForSelector('input[name="username"], input[placeholder*="Username"]', {
    timeout: 10_000,
  });
  const submit = page.getByRole("button", { name: /sign in/i });
  if (await submit.count()) await submit.click();
  // Wait for the shell — pick a landmark that's always present.
  await page.waitForLoadState("networkidle");
  await page.waitForTimeout(300);
}

async function ensureWorkspace(page, name) {
  // Try clicking the workspace switcher and selecting the named workspace.
  const switcher = page.getByRole("button", { name: /Switch workspace|Personal/ }).first();
  if (!(await switcher.count())) return;
  try {
    await switcher.click({ timeout: 2_000 });
    await page.waitForTimeout(200);
    const opt = page.getByRole("menuitem", { name }).first();
    if (await opt.count()) {
      await opt.click();
      await page.waitForTimeout(300);
    } else {
      // Close the menu so it doesn't show in the shot.
      await page.keyboard.press("Escape");
    }
  } catch {
    /* switcher not interactive on this viewport; ignore */
  }
}
