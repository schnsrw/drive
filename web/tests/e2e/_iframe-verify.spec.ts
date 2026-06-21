/**
 * Strict iframe verification — no permissive .or() chains, real console
 * error listeners, real visual mode assertions. If anything is broken
 * the test fails loud.
 */
import { expect, test } from "@playwright/test";

import { resetDemoState, signInDemo } from "./_helpers.ts";

const IGNORED_CONSOLE_PATTERNS: RegExp[] = [
  // React DevTools nag.
  /Download the React DevTools/i,
  // Vite HMR informational logs.
  /\[vite\]/i,
];

function shouldIgnore(text: string): boolean {
  return IGNORED_CONSOLE_PATTERNS.some((re) => re.test(text));
}

interface Capture {
  source: "console.error" | "pageerror";
  text: string;
}

function installErrorListener(page: import("@playwright/test").Page): Capture[] {
  const errors: Capture[] = [];
  page.on("console", (msg) => {
    if (msg.type() !== "error") return;
    const text = msg.text();
    if (shouldIgnore(text)) return;
    errors.push({ source: "console.error", text });
  });
  page.on("pageerror", (err) => {
    const text = err.message;
    if (shouldIgnore(text)) return;
    errors.push({ source: "pageerror", text });
  });
  return errors;
}

test.beforeEach(async ({ page }) => {
  await resetDemoState(page);
  await signInDemo(page);
});

test("templates fetch returns 200 (not 404)", async ({ page }) => {
  const xlsx = await page.request.get(`/templates/blank.xlsx`);
  expect(xlsx.status()).toBe(200);
  const docx = await page.request.get(`/templates/blank.docx`);
  expect(docx.status()).toBe(200);
});

test("embed runtimes are reachable at /embed/sheets and /embed/docs", async ({ page }) => {
  // Drive embeds BOTH editors via same-origin iframes; copy-embed copies
  // each SDK's runtime into public/embed/<app>/ at prebuild time.
  const sheetHtml = await page.request.get(`/embed/sheets/embed.html`);
  expect(sheetHtml.status()).toBe(200);
  const sheetRuntime = await page.request.get(`/embed/sheets/embed-runtime.js`);
  expect(sheetRuntime.status()).toBe(200);
  const docHtml = await page.request.get(`/embed/docs/embed.html`);
  expect(docHtml.status()).toBe(200);
  const docRuntime = await page.request.get(`/embed/docs/embed-runtime.mjs`);
  expect(docRuntime.status()).toBe(200);
});

test("create new .xlsx → card double-click routes to /file/<id> + editor iframe renders the grid", async ({ page }) => {
  const errors = installErrorListener(page);

  // Click New → New spreadsheet
  await page.getByRole("button", { name: /^New$/ }).click();
  await page.getByRole("menuitem", { name: /New spreadsheet/i }).click();

  // Wait for the success toast then wait for it to clear so it
  // doesn't intercept the file-card click.
  await expect(page.getByText(/Created Untitled spreadsheet/i)).toBeVisible({
    timeout: 5_000,
  });
  await expect(page.getByText(/Created Untitled spreadsheet/i)).toBeHidden({
    timeout: 8_000,
  });

  // Double-click the new file card — single click opens the preview
  // modal (every file type), double click opens the editor route.
  const card = page.locator(".cd-file-card").filter({ hasText: /Untitled spreadsheet/i });
  await card.scrollIntoViewIfNeeded();
  await card.dblclick();

  await expect(page).toHaveURL(/\/file\//, { timeout: 5_000 });

  // Sheet embeds via <iframe> in editor mode (Univer runs inside the
  // iframe runtime; Drive's app bundle carries no Univer).
  const iframe = page.getByTestId("casual-sheet-workspace");
  await expect(iframe).toBeVisible({ timeout: 15_000 });
  await expect(iframe).toHaveAttribute("src", /viewMode=editor/);

  // The blank.xlsx template parses, so the iframe actually paints Univer's
  // grid canvas — assert it renders inside, not just that the element exists.
  await page.waitForTimeout(3_000);
  const canvasCount = await page
    .frameLocator('[data-testid="casual-sheet-workspace"]')
    .locator("canvas")
    .count();
  expect(canvasCount).toBeGreaterThan(0);

  // No console errors / page errors during the mount.
  if (errors.length > 0) {
    throw new Error(
      `Browser captured ${errors.length} error(s) during editor mount:\n` +
        errors.map((e) => `  [${e.source}] ${e.text}`).join("\n"),
    );
  }
});

test("create new .docx → card double-click routes to /file/<id> + editor iframe", async ({ page }) => {
  const errors = installErrorListener(page);

  await page.getByRole("button", { name: /^New$/ }).click();
  await page.getByRole("menuitem", { name: /^New document$/i }).click();
  await expect(page.getByText(/Created Untitled/i)).toBeVisible({ timeout: 5_000 });
  await expect(page.getByText(/Created Untitled/i)).toBeHidden({ timeout: 8_000 });

  const card = page.locator(".cd-file-card").filter({ hasText: /Untitled \d+\.docx/i });
  await card.scrollIntoViewIfNeeded();
  await card.dblclick();

  await expect(page).toHaveURL(/\/file\//, { timeout: 5_000 });
  const iframe = page.getByTestId("casual-doc-editor");
  await expect(iframe).toBeVisible({ timeout: 10_000 });
  await expect(iframe).toHaveAttribute("src", /viewMode=editor/);

  await page.waitForTimeout(2_000);

  if (errors.length > 0) {
    throw new Error(
      `Browser captured ${errors.length} error(s) during editor mount:\n` +
        errors.map((e) => `  [${e.source}] ${e.text}`).join("\n"),
    );
  }
});

test("single-click .xlsx → preview modal embeds the iframe in viewMode=preview (read-only)", async ({ page }) => {
  // Drive's contract: preview ⇒ viewMode="preview", which the SDK
  // enforces as read-only. Drive does NOT hand-roll a read-only guard —
  // it just passes the mode and trusts the SDK. Here we lock in that Drive
  // sends the right mode; SDK-side read-only enforcement is verified in the
  // sheet SDK's own suite.
  await page.getByText("Q2 planning.xlsx").first().click();
  const iframe = page.getByTestId("casual-sheet-workspace");
  await expect(iframe).toBeVisible({ timeout: 15_000 });
  await expect(iframe).toHaveAttribute("src", /viewMode=preview/);
});

test("card double-click on a .xlsx → /file/<id> mounts the editor iframe in editor mode", async ({ page }) => {
  await page.getByRole("button", { name: /^New$/ }).click();
  await page.getByRole("menuitem", { name: /New spreadsheet/i }).click();
  await expect(page.getByText(/Created Untitled spreadsheet/i)).toBeHidden({
    timeout: 8_000,
  });

  const card = page.locator(".cd-file-card").filter({ hasText: /Untitled spreadsheet/i });
  await card.scrollIntoViewIfNeeded();
  await card.dblclick();

  await expect(page).toHaveURL(/\/file\//, { timeout: 5_000 });

  // Iframe embed in editor mode (viewMode=editor in the iframe src).
  const iframe = page.getByTestId("casual-sheet-workspace");
  await expect(iframe).toBeVisible({ timeout: 15_000 });
  await expect(iframe).toHaveAttribute("src", /viewMode=editor/);
});
