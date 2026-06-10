/**
 * SR15 — Playwright baseline for keystroke→paint latency.
 * Spec: docs/ux/12-search-surface.md §"Performance budget".
 *
 * The SPA's lib/searchMetrics.ts opens a measurement window on every
 * first-keystroke-after-paint and closes it in a double-rAF after
 * the search effect setStates the result pane. Stats are surfaced
 * on `window.__cd_search_perf()`.
 *
 * This spec types a few real queries against the demo backend, reads
 * the rolling buffer, and asserts:
 *   - At least one measurement landed (the wiring is connected).
 *   - p95 is under a 500 ms ceiling.
 *
 * Sized as ~2× the local baseline (Mac p95 ~240 ms) to absorb CI
 * variance. The spec target is 200 ms; we're not asserting tighter
 * yet because the demo backend adds ~50 ms over a real OpenSearch
 * round-trip and CI runners vary widely. If we see a few weeks of
 * green at <500 ms in CI, halve the ceiling.
 */
import { expect, test } from "@playwright/test";
import { resetDemoState, signInDemo } from "./_helpers.ts";

type PerfStats = { count: number; p50_ms: number; p95_ms: number; max_ms: number } | null;

test.beforeEach(async ({ page }) => {
  await resetDemoState(page);
  await signInDemo(page);
});

test("search keystroke→paint latency stays under the permissive ceiling", async ({ page }) => {
  const search = page.getByPlaceholder("Search files and folders");

  // Exercise the search pipeline a few times against the seeded demo
  // corpus. Each iteration: clear, type, wait for visible result, blur.
  for (const q of ["plan", "Q2", "design"]) {
    await search.click();
    await search.fill("");
    await search.fill(q);
    // Debounce is 50 ms; allow slack so the double-rAF + paint
    // closes the measurement before we read. 300 ms covers the
    // 50 ms debounce + ~200 ms paint budget + headroom.
    await page.waitForTimeout(300);
    await search.blur();
  }

  const stats: PerfStats = await page.evaluate(() => {
    const fn = (window as unknown as { __cd_search_perf?: () => PerfStats }).__cd_search_perf;
    return fn ? fn() : null;
  });

  // Diagnostic output — surfaced in CI logs so we accumulate a few
  // runs of baseline numbers before tightening the ceiling toward the
  // spec's 200 ms target.
  console.log("SR15 search perf stats:", JSON.stringify(stats));

  expect(stats, "instrumentation should have captured samples").not.toBeNull();
  if (!stats) return; // narrow for TS
  expect(stats.count, "at least one keystroke→paint pair landed").toBeGreaterThanOrEqual(1);
  // 500 ms ceiling — 2× local baseline (p95 ~240 ms) to absorb CI
  // variance. Tighten toward the spec's 200 ms after a few weeks of
  // green CI numbers.
  expect(stats.p95_ms, "p95 within ceiling").toBeLessThan(500);
});
