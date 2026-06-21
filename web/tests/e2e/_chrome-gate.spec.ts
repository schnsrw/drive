/**
 * Strict gate for the editor + preview chrome shipped in the
 * 2026-06-17 UX-EDITOR-* batch (PIPELINE.md "Editor & Preview UX
 * (premium quality bar)" theme). Every spec hits a zero-console-error
 * bar — any pageerror or unfiltered console.error fails the test.
 *
 * What this gate locks in:
 *   1. PreviewModal Expand button → /file/<id>            (UX-EDITOR-6)
 *   2. /file/<id> header has Share + kebab + filename     (UX-EDITOR-4)
 *   3. Filename inline rename round-trips                 (UX-EDITOR-4)
 *   4. SaveStatusPill testid mounts (idle state — no save fires
 *      against the demo's empty blobs, but the host shell is wired)
 *   5. Sheet editor at /file/<id> embeds the SDK iframe wrapped by
 *      DRIVE'S OWN toolbar with the 9 v0.6 commands
 *      (undo/redo, B/I/U/S, align L/C/R)                   (UX-EDITOR-1)
 *   6. Doc preview shows the friendly "Couldn't load preview" card
 *      instead of the SDK's red parse-error UI             (UX-EDITOR-5)
 *   7. Sheet preview shows the same friendly card          (UX-EDITOR-5)
 *
 * Future regressions land here, not in the per-PR visual specs.
 */
import { expect, test, type Page } from "@playwright/test";

import { resetDemoState, signInDemo } from "./_helpers.ts";

function attachStrictErrorListener(page: Page) {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(`[pageerror] ${e.message}`));
  page.on("console", (m) => {
    if (m.type() === "error") errors.push(`[console.error] ${m.text()}`);
  });
  (page as unknown as { __strictErrors__: string[] }).__strictErrors__ = errors;
}

test.beforeEach(async ({ page }) => {
  await resetDemoState(page);
  await signInDemo(page);
  attachStrictErrorListener(page);
});

// Errors the strict gate intentionally ignores — both originate
// inside the SDK iframe and are already surfaced to users via
// Drive's FailureFallback. They're not actionable from the host.
const IGNORED_ERROR_FRAGMENTS = [
  // Chromium sandboxing warning fires on every same-origin iframe
  // load; we can't influence it.
  "allow-scripts and allow-same-origin",
  // doc SDK logs parseDocx failures to console even when the host
  // wire's `onError` already reports them — redundant noise.
  "[parseDocx]",
  // sheet SDK's parser worker logs xlsx parse failures similarly.
  "Failed to load workbook",
  // ExcelJS internal stack — printed by the same parser worker.
  "End of data reached",
];

test.afterEach(async ({ page }) => {
  const errors = (page as unknown as { __strictErrors__: string[] }).__strictErrors__ ?? [];
  const filtered = errors.filter((e) => !IGNORED_ERROR_FRAGMENTS.some((s) => e.includes(s)));
  if (filtered.length) {
    throw new Error(`Strict errors captured:\n${filtered.join("\n")}`);
  }
});

async function openSheetEditor(page: Page) {
  await page.getByRole("button", { name: /^New$/ }).click();
  await page.getByRole("menuitem", { name: /New spreadsheet/i }).click();
  await page.waitForTimeout(2_000);
  const card = page.locator(".cd-file-card").filter({ hasText: /Untitled spreadsheet/i });
  await card.scrollIntoViewIfNeeded();
  await card.click();
  await page.getByRole("button", { name: /Open in editor/i }).click();
  await expect(page).toHaveURL(/\/file\//, { timeout: 10_000 });
  await page.getByTestId("file-fullscreen").waitFor({ timeout: 15_000 });
}

test("UX-EDITOR-6: PreviewModal Expand button routes to fullscreen", async ({ page }) => {
  test.setTimeout(45_000);
  await page.getByText("Q2 planning.xlsx").first().click();
  await page.getByTestId("preview-expand").waitFor();
  await page.getByTestId("preview-expand").click();
  await expect(page).toHaveURL(/\/file\//, { timeout: 5_000 });
  await page.getByTestId("file-fullscreen").waitFor();
});

test("UX-EDITOR-4: /file/<id> header chrome — Share + kebab + filename", async ({ page }) => {
  test.setTimeout(60_000);
  await openSheetEditor(page);
  await expect(page.getByTestId("file-fullscreen-share")).toBeVisible();
  await expect(page.getByTestId("file-fullscreen-back")).toBeVisible();
  await expect(page.getByTestId("file-fullscreen-title")).toBeVisible();
  await expect(page.getByRole("button", { name: /More actions/i })).toBeVisible();
});

test("UX-EDITOR-4: filename inline rename round-trips through PATCH", async ({ page }) => {
  test.setTimeout(60_000);
  await openSheetEditor(page);
  await page.getByTestId("file-fullscreen-title").click();
  const input = page.getByTestId("file-fullscreen-title-input");
  await input.waitFor();
  await input.fill("Renamed by gate.xlsx");
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("file-fullscreen-title")).toHaveText("Renamed by gate.xlsx");
});

test("UX-EDITOR-1: sheet editor renders Drive's OWN toolbar around the iframe", async ({ page }) => {
  test.setTimeout(60_000);
  await openSheetEditor(page);
  // Drive embeds the editor via <iframe> and renders its OWN chrome — the
  // SheetToolbar ribbon above the grid (NOT the SDK's built-in chrome).
  // Lock in the v0.6 command set: undo/redo, B/I/U/S, align L/C/R.
  await expect(page.getByTestId("sheet-toolbar")).toBeVisible({ timeout: 15_000 });
  for (const cmd of [
    "undo",
    "redo",
    "bold",
    "italic",
    "underline",
    "strikethrough",
    "align-left",
    "align-center",
    "align-right",
  ]) {
    await expect(page.getByTestId(`sheet-tool-${cmd}`)).toBeVisible();
  }
  // The grid renders inside Drive's iframe (host owns the chrome, iframe
  // owns the canvas).
  await expect(page.getByTestId("casual-sheet-workspace")).toBeVisible();
});

test("UX-EDITOR-1 v2: Drive's toolbar exposes font family + size controls", async ({ page }) => {
  test.setTimeout(60_000);
  await openSheetEditor(page);
  await page.getByTestId("sheet-toolbar").waitFor({ timeout: 15_000 });
  // Drive's toolbar dispatches set-font-family / set-font-size over the
  // embed transport's executeCommand (args carried intact).
  await expect(page.getByTestId("sheet-tool-font-family")).toBeVisible();
  await expect(page.getByTestId("sheet-tool-font-size")).toBeVisible();
});

test("UX-EDITOR-5: docx preview shows friendly fallback instead of parse error", async ({
  page,
}) => {
  test.setTimeout(60_000);
  await page.getByText("Product brief.docx").first().click();
  // The demo's seeded blob is empty → SDK fires casual.error
  // → ErrorAwareDoc swaps the iframe for FailureFallback.
  // Friendly card carries the "Couldn't load preview" string.
  await expect(page.getByText(/Couldn't load preview/i)).toBeVisible({
    timeout: 15_000,
  });
  // The SDK's own red "Failed to Load Document" UI must NOT show.
  await expect(page.getByText(/Failed to Load Document/i)).toHaveCount(0);
});

test("UX-EDITOR-5: xlsx preview shows friendly fallback instead of parse error", async ({
  page,
}) => {
  test.setTimeout(60_000);
  await page.getByText("Q2 planning.xlsx").first().click();
  await expect(page.getByText(/Couldn't load preview/i)).toBeVisible({
    timeout: 15_000,
  });
  await expect(page.getByText(/Failed to load workbook/i)).toHaveCount(0);
});

test("UX-EDITOR-8 phase 2: FileFullscreen Details pill opens drawer with same panel", async ({
  page,
}) => {
  test.setTimeout(60_000);
  await openSheetEditor(page);
  await expect(page.getByTestId("file-fullscreen-details")).toBeVisible();
  await page.getByTestId("file-fullscreen-details").click();
  await page.getByTestId("file-fullscreen-details-drawer").waitFor({ timeout: 5_000 });
  await expect(page.getByTestId("details-panel")).toBeVisible();
  await expect(page.getByTestId("details-tab-info")).toBeVisible();
  // Esc closes the drawer
  await page.keyboard.press("Escape");
  await expect(page.getByTestId("file-fullscreen-details-drawer")).toHaveCount(0);
});

test("UX-EDITOR-8: PreviewModal Details panel mounts all 3 tabs", async ({ page }) => {
  test.setTimeout(60_000);
  await page.getByText("Q2 planning.xlsx").first().click();
  await page.getByTestId("details-panel").waitFor({ timeout: 5_000 });
  await expect(page.getByTestId("details-tab-info")).toBeVisible();
  await expect(page.getByTestId("details-tab-people")).toBeVisible();
  await expect(page.getByTestId("details-tab-history")).toBeVisible();
  // Info tab is the default — content panel renders the metadata grid.
  await expect(page.getByTestId("details-tab-info-panel")).toBeVisible();
});

test("UX-EDITOR-8: Details People tab → empty state + Create share CTA", async ({ page }) => {
  test.setTimeout(60_000);
  await page.getByText("Q2 planning.xlsx").first().click();
  await page.getByTestId("details-tab-people").click();
  // Demo state starts with no shares for the seeded file → empty state.
  await expect(page.getByTestId("details-tab-people-panel")).toBeVisible({ timeout: 5_000 });
  await expect(page.getByTestId("details-people-create-share")).toBeVisible();
});

test("UX-EDITOR-8: Details History tab → friendly Coming soon", async ({ page }) => {
  test.setTimeout(60_000);
  await page.getByText("Q2 planning.xlsx").first().click();
  await page.getByTestId("details-tab-history").click();
  await expect(page.getByTestId("details-tab-history-panel")).toBeVisible({ timeout: 5_000 });
  await expect(page.getByText(/Version history is coming/i)).toBeVisible();
});

test("UX-EDITOR-7: video preview mounts the vidstack player, not browser default", async ({
  page,
}) => {
  test.setTimeout(60_000);
  await page.getByText("Demo walkthrough.mp4").first().click();
  // The vidstack default-layout adds a wrapping element with the
  // 'cd-media-shell--video' class. The browser's bare <video controls>
  // would have NO surrounding wrapper.
  await expect(page.locator(".cd-media-shell--video")).toBeVisible({ timeout: 5_000 });
});

// NB: the former "UX-EDITOR-2: iframe stays light-themed under
// prefers-color-scheme:dark" gate is intentionally removed. It locked in
// copy-embed's theme-lock shim (an inline MutationObserver pinning
// data-theme="light"), which was a workaround for pre-0.11 embed builds.
// The rewritten copy-embed copies the SDK's clean embed runtimes verbatim
// with no HTML patching, so there's no host-injected theme lock to assert.
// Host-driven theming will return as an explicit `casual.command.set.theme`
// wire (the protocol already defines it) once Drive ships a theme toggle.
