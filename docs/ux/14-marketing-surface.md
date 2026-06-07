# 14 — Marketing site surface

Companion to `docs/research/07-marketing-site.md`. Page-by-page surface spec for the Astro marketing site at `casualoffice.org` (initially `<gh-org>.github.io/drive/`).

## Flows

1. **Discover** — visitor lands on `/` from search/HN. Hero conveys what Drive is + a Demo CTA + a Self-host CTA in the first viewport.
2. **Try** — Demo CTA → `/demo` → SPA loads in demo mode with the seeded admin already signed in. No setup screen, no real password.
3. **Install** — Self-host CTA → `/docs/install` → copy/paste Docker one-liner + cargo path. Returns to `/` via top nav.
4. **Configure** — `/docs/configuration` lists every env var (storage backend, two-origin hosts, SMTP, etc.) with a sane default and an example.
5. **Understand** — `/docs/architecture` shows the high-level diagram + the three-token identity model + WOPI handoff sequence.
6. **Contribute** — `/docs/contributing` covers repo layout, dev loop, PR conventions, the five inviolable rules (linked back to `CLAUDE.md`).
7. **Browse** — `/screenshots` is a lightboxable gallery; each shot has a one-line caption.
8. **Return** — Footer surfaces GitHub repo, Discussions, license, social.

## Global chrome

```
┌─ Top nav (sticky on desktop, collapse-to-hamburger on mobile) ───┐
│  [Logo] Casual Drive       Docs ▾   Screenshots   Demo   GitHub  │
│                            │                                      │
│                            └─ Install / Configuration /           │
│                               Architecture / Contributing         │
└──────────────────────────────────────────────────────────────────┘

… page body …

┌─ Footer ──────────────────────────────────────────────────────────┐
│  Casual Drive · part of Casual Office · MIT                       │
│  Repo   Discussions   Issues   Releases   Sheet ↗   Document ↗    │
│  © 2026 — schnsrw.live                                            │
└──────────────────────────────────────────────────────────────────┘
```

- Top nav background blurs on scroll (`backdrop-filter: blur(8px)`).
- Theme toggle on far right of nav (icon-only, Lucide `Sun`/`Moon`).
- Skip-link `Skip to content` on first focus.

## Mobile chrome (≤ 640 px)

```
┌─ Top nav ─────────────────────────────────────────┐
│  [Logo] Casual Drive                       [Menu] │
└───────────────────────────────────────────────────┘
            ↓ tap menu
┌─ Sheet (vaul drawer, slides up) ──────────────────┐
│  Docs                                             │
│   Install                                         │
│   Configuration                                   │
│   Architecture                                    │
│   Contributing                                    │
│  Screenshots                                      │
│  Demo                                             │
│  GitHub  ↗                                        │
│  ──────                                           │
│  Theme  ◐                                         │
└───────────────────────────────────────────────────┘
```

- Drawer is `vaul`-style sheet from the bottom, snap to 50/100%.
- Tap targets 44 × 44 minimum.

## `/` — landing

```
┌─ Hero (full-bleed, gradient fade) ────────────────────────────────┐
│                                                                    │
│   Your files. Your editors. Your server.                          │
│   Casual Drive is an open-source, self-hosted Drive that          │
│   opens .xlsx and .docx in the browser — no Google account        │
│   required.                                                        │
│                                                                    │
│   [ Try the demo ]   [ Self-host in 30 seconds ]                  │
│                                                                    │
│   ★ MIT  ·  Rust + React  ·  Single binary  ·  Docker-ready       │
│                                                                    │
└────────────────────────────────────────────────────────────────────┘
┌─ Screenshot showcase (autoplay or hover-cycle) ───────────────────┐
│   [ wide screenshot — Files list, dark theme ]                    │
│   Files   ·   Editor   ·   Sharing   ·   Mobile                   │
└────────────────────────────────────────────────────────────────────┘
┌─ Feature grid (3-up desktop, 1-up mobile) ────────────────────────┐
│ [ico] Open in browser     [ico] Self-host       [ico] Cmd-K       │
│ .xlsx + .docx via WOPI    fs / S3 / MinIO       global search     │
│                                                                    │
│ [ico] Two-origin model    [ico] Audit log       [ico] No tracking │
│ usercontent isolation     every action logged   zero analytics    │
└────────────────────────────────────────────────────────────────────┘
┌─ How it works (3 steps) ───────────────────────────────────────────┐
│   1. docker run …                                                  │
│   2. Open https://drive.your-server                                │
│   3. Upload a file. Click it. Edit it.                             │
└────────────────────────────────────────────────────────────────────┘
┌─ Comparison table (Casual Drive vs Drive vs Nextcloud) ───────────┐
│   ... table ...                                                    │
└────────────────────────────────────────────────────────────────────┘
┌─ Final CTA ────────────────────────────────────────────────────────┐
│   Ready?  [ Demo ]  [ Install ]  [ ★ Star on GitHub ]              │
└────────────────────────────────────────────────────────────────────┘
```

- Hero `<h1>` is the only `<h1>` on the page.
- Screenshot showcase uses `<Image>` with `loading="eager"` for the first frame, `loading="lazy"` for the rest.
- Feature grid is a `display: grid`; mobile = `grid-template-columns: 1fr`, sm = `1fr 1fr`, lg = `1fr 1fr 1fr`.
- Comparison table on mobile collapses to "horizontal-scroll inside container" — the table itself never overflows the viewport.

## `/docs/install`

- Two install paths: Docker (recommended) + `cargo install` (advanced).
- Each path is a code block with a one-line copy button. Code blocks use Astro Shiki (compile-time syntax highlighting, no client JS).
- Required env vars listed inline with one-line explanation each, linking to `/docs/configuration` for the full table.
- "First-run checklist" section: visit `https://drive.your-server`, complete admin setup, upload a file, click it.

## `/docs/configuration`

- One big table of env vars: name, default, example, notes.
- Sections: Bind & origins · Storage backend · Database · Sessions · Rate limits · Editor handoff (sheet/document URLs) · SMTP (deferred).
- Per-backend storage subsection: filesystem (one env var) · S3 (5 env vars) · MinIO (S3 with custom endpoint) · in-memory (testing).

## `/docs/architecture`

- Embedded SVG diagram (hand-authored, not auto-generated) showing: browser ↔ drive.host ↔ usercontent-drive.host ↔ Storage adapter ↔ {fs, S3, MinIO}.
- Three-token identity model section.
- WOPI handoff sequence (numbered steps with mini ASCII or SVG sequence diagram).
- Each major section links back to the relevant `docs/research/` brief.

## `/docs/contributing`

- Repo layout (tree).
- Dev loop: `cargo run -p drive` + `cd web && pnpm dev` + `cd marketing && pnpm dev`.
- PR conventions: small, focused, tests included, docs updated in the same commit.
- The five inviolable rules summarised + linked back to `CLAUDE.md`.
- "Where to start" list of `good-first-issue` labelled GitHub issues.

## `/screenshots`

- Gallery grid (2-col mobile, 3-col tablet, 4-col desktop) of every flagship surface.
- Each tile = `<Image>` + caption. Click → lightbox (Astro island, hydrates `client:visible`).
- Sections: Files · Editor (Sheet) · Editor (Document) · Sharing · Settings · Admin · Mobile.

## `/demo`

```
┌─ Slim header (sticky) ───────────────────────────────────────────────┐
│  [Logo] Casual Drive · Demo                  Reset · Back to docs    │
└──────────────────────────────────────────────────────────────────────┘
┌─ Drive SPA (full viewport below header) ─────────────────────────────┐
│                                                                      │
│   …                                                                  │
│   (the actual SPA running in demo mode with seeded data)             │
│   …                                                                  │
│                                                                      │
└──────────────────────────────────────────────────────────────────────┘
```

- Page `<head>` has `<meta name="robots" content="noindex, nofollow">`.
- The SPA bundle lives at `/demo-app/` (static, copied from `web/dist/` at build time).
- The Astro `/demo` page is a thin host: header + an `<iframe>` (or full-screen redirect) pointing at `/demo-app/index.html`.
- "Reset" wipes demo `localStorage` keys (`cd-workspace-id-v1`, demo seed flag) and reloads.
- "Back to docs" is a plain link to `/docs/install`.

## State checklists per page

- **Empty:** every list/section has a real-content stub; no "TBD".
- **Loading:** skeletons (commandment 6). Marketing pages are static, so the only loaders live on `/demo` (handled by the SPA).
- **Error:** if a screenshot 404s the `<Image>` falls back to its `alt` text in a styled box, not a broken-image icon.
- **No-JS:** every page renders correctly. The /demo page shows a "JavaScript required to run the demo" message in `<noscript>`.
- **Reduced motion:** all transitions ≥ 200ms are gated by `prefers-reduced-motion: no-preference`.

## Polish bar (10 commandments, marketing context)

1. **One primary action per screen** — Hero has 2 CTAs but the *primary* (filled) is Demo; Install is the secondary (outline).
2. **Type carries hierarchy** — h1 4xl → h2 2xl → body lg → meta sm. No bold-as-hierarchy.
3. **Snap to 4/8 grid** — same spacing scale as Drive.
4. **Concentric corners** — outer container rounding matches inner card rounding.
5. **Sub-100 ms** — interactions (theme toggle, nav drawer) feel instant.
6. **Skeletons not spinners** — N/A; static pages.
7. **Keyboard is first-class** — nav fully tab-navigable; theme toggle has visible focus ring; skip-link works.
8. **`prefers-reduced-motion`** — honoured on hero parallax, screenshot fade, drawer slide.
9. **One icon family** — Lucide everywhere.
10. **Copy is warm, direct, sentence-case** — no all-caps headings, no `!`, no exclamatory marketing prose.
