/**
 * SDK integration smoke — sheet + doc editors mounted inside Drive.
 *
 * ED1 (sheet/doc Phase 1) — verifies:
 *  - The PreviewModal opens .docx / .xlsx and reaches the SDK mount.
 *  - "Open in editor" navigates to `/file/<id>` (the new fullscreen
 *    route from ED1 gap a).
 *  - `/file/<id>` cold-loaded (no `history.state`) surfaces the
 *    "open from your file list" message — the explicit error path.
 *  - The back arrow returns to `/`.
 *
 * What this does NOT cover (deferred to fixture-rich later passes):
 *  - Actual rendering of valid .docx / .xlsx bytes. Demo seeds carry
 *    empty Blobs for the seeded fixtures, so the parse path inside
 *    the SDK reaches its error UI rather than a populated grid /
 *    document. Real-byte rendering tests need a fixtures pipeline
 *    (Phase 1.5).
 *  - Co-edit. That's task #124 — multi-container compose.
 */
import { expect, test } from "@playwright/test";

import { resetDemoState, signInDemo } from "./_helpers.ts";

test.beforeEach(async ({ page }) => {
  await resetDemoState(page);
  await signInDemo(page);
});

test("preview .xlsx → primary action navigates to /file/<id> with editor chrome", async ({ page }) => {
  // The demo seeds a 'Q2 planning.xlsx' file at the workspace root.
  await page.getByText("Q2 planning.xlsx").first().click();
  // The PreviewModal mounts CasualSheetWorkspace via PreviewStage.
  // It immediately fires through DriveFileSource → xlsxToWorkbookData.
  // For the empty seeded Blob the loader may resolve to an error or
  // a near-empty workbook — either way the host wrapper renders one
  // of its known testids (loading / ready / error).
  const stage = page.getByTestId("sheet-workspace").or(
    page.getByTestId("sheet-workspace-loading").or(
      page.getByTestId("sheet-workspace-error"),
    ),
  );
  await expect(stage).toBeVisible({ timeout: 15_000 });

  // Click "Open in editor" — should navigate to /file/<id> and
  // mount FileFullscreen with editor chrome (mode=editor).
  await page.getByRole("button", { name: /Open in editor/i }).click();
  await expect(page).toHaveURL(/\/file\//, { timeout: 5_000 });
  await expect(page.getByTestId("file-fullscreen")).toBeVisible();
  await expect(page.getByTestId("file-fullscreen-title")).toHaveText("Q2 planning.xlsx");

  // FileFullscreen mounted a CasualSheetWorkspace. Accept any of the
  // three resolved states (ready / loading / error) — the demo's
  // seeded empty Blob can't parse, so 'error' is the expected
  // terminal state; the parser worker hops through 'loading' first.
  const fullscreenStage = page
    .getByTestId("sheet-workspace")
    .or(page.getByTestId("sheet-workspace-loading"))
    .or(page.getByTestId("sheet-workspace-error"));
  await expect(fullscreenStage).toBeVisible({ timeout: 15_000 });
});

test("preview .docx → primary action navigates to /file/<id>", async ({ page }) => {
  await page.getByText("Product brief.docx").first().click();
  // PreviewStage mounts CasualDocEditor for kind='doc'. Wait for
  // the editor's outer wrapper (the lazy chunk has to land first).
  await expect(page.getByText(/Loading editor…|Couldn't open|Casual/i).first()).toBeVisible({
    timeout: 15_000,
  });

  await page.getByRole("button", { name: /Open in editor/i }).click();
  await expect(page).toHaveURL(/\/file\//, { timeout: 5_000 });
  await expect(page.getByTestId("file-fullscreen")).toBeVisible();
  await expect(page.getByTestId("file-fullscreen-title")).toHaveText("Product brief.docx");
});

test("/file/<id> cold load fetches metadata via GET /api/files/{id}", async ({ page }) => {
  // Navigate via history.pushState rather than page.goto — addInitScript
  // (used by resetDemoState) re-fires on every fresh document and wipes
  // the auth state. Same-document push keeps us authed.
  await page.evaluate(() => {
    window.history.pushState(null, "", "/file/f_quarter");
    window.dispatchEvent(new PopStateEvent("popstate"));
  });
  // FileFullscreen now hits GET /api/files/{id} (backed by demo's
  // route handler) and resolves to the seeded 'Q2 planning.xlsx'.
  await expect(page.getByTestId("file-fullscreen-title")).toHaveText("Q2 planning.xlsx", {
    timeout: 5_000,
  });
});

test("/file/<unknown> cold load surfaces the not-found error", async ({ page }) => {
  await page.evaluate(() => {
    window.history.pushState(null, "", "/file/does-not-exist");
    window.dispatchEvent(new PopStateEvent("popstate"));
  });
  await expect(page.getByTestId("file-fullscreen-error")).toBeVisible({ timeout: 5_000 });
});

test("back arrow returns from /file/<id> to /", async ({ page }) => {
  await page.getByText("Q2 planning.xlsx").first().click();
  await page.getByRole("button", { name: /Open in editor/i }).click();
  await expect(page).toHaveURL(/\/file\//, { timeout: 5_000 });
  await page.getByTestId("file-fullscreen-back").click();
  await expect(page).toHaveURL(/\/(\?.*)?$/);
  // Shell renders again — its top bar's "New" button is a stable
  // signal we're back at the file picker.
  await expect(page.getByRole("button", { name: /^New$/ })).toBeVisible();
});

test("document.title tracks the open file's name", async ({ page }) => {
  await page.getByText("Q2 planning.xlsx").first().click();
  await page.getByRole("button", { name: /Open in editor/i }).click();
  await expect(page).toHaveURL(/\/file\//, { timeout: 5_000 });
  // Tab title flips to include the filename. (The fullscreen page
  // restores the prior title on unmount.)
  await expect(page).toHaveTitle(/Q2 planning\.xlsx/);
});
