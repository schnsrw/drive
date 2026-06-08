# 17 — Notes UX for general users (Phase 3)

The notes app that shipped in v0 ([`09-notes-wiki`](./09-notes-wiki.md) + [`16-notes-surface`](../ux/16-notes-surface.md)) is a *developer's* notes app: a `<textarea>` of markdown source on the left, a rendered preview on the right, `[[Page name]]` typed by hand for links, `Tab` for indent. Engineers write README-shaped notes there comfortably; a non-technical user opens it, sees `# heading` and `**bold**` as text instead of formatting, and stops.

For Drive to be the file home of a real team — design, ops, finance, leadership — Notes has to feel like a tool *they* would pick up, not one they tolerate because the engineer on the team set it up. This brief locks the shape of that pivot.

## Why now

Three signals pushed this up the queue:

1. **Notes is the first surface a non-technical user touches after sign-in.** Files are universal (drag, drop, open); editor handoff is one click. Notes is the one place we ask the user to *write*, and the current experience asks them to write *in markdown source*.
2. **The "export = `cat` it" guarantee can be kept with a different editor.** Tiptap (ProseMirror) and Lexical both have a markdown-storage path. The argument for keeping the source-pane UX was storage portability — that argument doesn't survive once a WYSIWYG editor serializes to the same markdown.
3. **The wiki-link pattern is invisible to general users.** Typing literal `[[` to make a link is a power-user move. Notion taught the world that typing `@` or `/` to open a picker is the actual general-user gesture.

The current shape is correct for *capturing* the data; it's wrong for *acquiring* the user.

## The bar: premium, adopted from the best

We are not inventing a new notes UX. Premium notes is a solved category; the work is to *adopt* the patterns the leaders have converged on and execute them at the polish bar in [`04-polish-principles`](./04-polish-principles.md). The specific references we benchmark against:

- **Obsidian Live Preview** — the gold standard for "markdown-on-disk, never visible to the user." Every locked decision below should pass the test "would this feel natural to an Obsidian Live Preview user?"
- **macOS Notes** — the aesthetic restraint floor. Chrome should disappear; the document should breathe. No floating panels, no toolbars-by-default, no sidebars that demand attention.
- **Mem** — the modern "AI-native, but the AI is invisible until invoked" posture. We aren't shipping AI in this brief, but the *posture* (powerful features that don't clutter the surface) is what we copy.
- **Bear** — the live-render-markdown reference implementation on contenteditable.

Where these tools disagree, we pick the choice that is **kindest to a non-technical user on first contact**. Where they agree, we follow without debate.

## Audit of what ships today

| Current UX | Why it reads as developer-shaped |
|---|---|
| Markdown source pane (left) + rendered preview (right) | Most users have never seen the source/preview split outside of `git` README work. The preview is also "the document," so showing the source as an equal-weight pane confuses the model. |
| `# heading`, `**bold**`, `- item` typed by hand | Markdown syntax is invisible cognitive load. A general user expects `Cmd-B` to bold, not `**…**`. |
| `[[Page name]]` typed in literal double brackets | This is a wiki convention from MediaWiki / Obsidian / Logseq. Notion taught general users that links happen on `@` or `/`. |
| Tab-to-indent in the source pane | A general user pressing `Tab` expects to leave the field, not to indent text. |
| `Cmd-S` to force-flush save | Auto-save was added; the muscle-memory shortcut is a holdover from the dev mental model. (Keep `Cmd-S` for the dev who looks for it — but it shouldn't be the *only* save signal.) |
| Backlinks panel labelled "Linked from" with a horizontal rule above it | The wording + the affordance is wiki-native; a general user doesn't model documents as "linked from." Notion calls this "Mentions" + buries it. |

The substrate (markdown storage, `note_links` table, workspace scoping, tree, search) is all fine. The **editor and the gestures around it** are what need to change.

## Pattern survey

I looked at the tools a non-technical user actually picks up. Six reference points:

| Tool | Editor style | Slash menu? | Link picker | Markdown storage? | Lesson |
|---|---|---|---|---|---|
| **Notion** | WYSIWYG block, drag handles on every block | yes (`/`) | `@` (people/pages) or `[[` autocomplete | proprietary JSON; markdown export | Slash + `@` are the canonical gestures. Drag handles tell users a block is a unit. |
| **Bear** | WYSIWYG that renders markdown live as you type (hybrid) | no | `[[` autocomplete | yes, native markdown | Live-render keeps file portability without showing the source. Good middle ground. |
| **Apple Notes** | pure WYSIWYG, no markdown | no | proprietary "wiki" link via `>` autocomplete (in 14+) | no; iCloud blobs | The bar for "feels obvious" with zero learning curve. Useful as the target *floor*. |
| **Craft** | WYSIWYG block, drag handles | yes (`/`) | `[[` autocomplete | proprietary; markdown export | Modern Notion alternative; same patterns. |
| **Capacities** / **Anytype** | WYSIWYG block, P2P | yes | `@` and `[[` | proprietary | Power-user end of the spectrum. We don't go here. |
| **Obsidian** | markdown source with live-render mode toggle | yes (in plugins) | `[[` autocomplete | yes, plain markdown files on disk | Closest analogue to where we are today. Used overwhelmingly by developers. Not our target audience. |

The pattern that maps onto our existing markdown storage AND matches the experience the user pointed at — **Obsidian Live Preview / macOS Notes / Mem / Bear** — is:

**Live-render markdown, never expose the source.** The user types in a single pane. The moment markdown syntax becomes unambiguous (`# ` + space, `**word**`, `- ` at line start, `> `), it visibly *becomes* the formatted thing. The `#` / `**` / `-` either disappears entirely or fades to a subtle gutter marker that only appears on the active line. There is no preview pane. There is no source view. The editor *is* the document.

Obsidian's Live Preview mode and Bear both ship this exact UX over plain-markdown storage. Apple Notes and Mem go one step further (pure WYSIWYG, no markdown shortcuts at all) but lose the "type `# ` to get a heading" affordance that power-users like. The middle ground — live-render with markdown shortcuts as muscle-memory accelerators — keeps both audiences served from one surface.

Notion's block editor is the *alternative* pattern: WYSIWYG with mandatory slash commands + drag handles + per-block toolbars. It's familiar to a different (younger, web-native) audience but is heavier to ship and over-structures simple notes. We're not building that. We're building Obsidian Live Preview's behaviour with macOS Notes' aesthetic restraint.

## Locked decisions

### **Editor: Tiptap (ProseMirror) with markdown serialization + live-render schema**

- Tiptap is the cleanest "live-render markdown on contenteditable" library on the web — Bear-on-the-Mac and Obsidian-Live-Preview both rely on ProseMirror-shaped editors; Tiptap is the modern React wrapper.
- Markdown round-trip via `prosemirror-markdown` (Tiptap ships a wrapper) — the on-disk format stays `text/markdown`, no migration of existing notes.
- The existing `marked` + `dompurify` renderer becomes the **share-link / public render** path only — not the in-app editor.
- Bundle hit ~120 KB; route-split so Files / Cmd-K / sign-in stay unchanged.
- Key extension config: keep ProseMirror's input-rules (for the markdown shortcuts) + Tiptap's selection menu + Tiptap's slash-command extension. Disable Tiptap's "rich paste" HTML-from-Word importer initially — we want clean markdown round-trip first.

### **Single pane. Live-render markdown. No source ever visible.**

- One pane. The editor *is* the document.
- The preview pane that exists today is removed.
- The edit/preview tab control on mobile is removed.
- There is **no** "view source" toggle. A general user never sees `# heading` or `**bold**` as raw characters. (Engineers who want to inspect the markdown can `cat` the note from disk / the bucket — the storage format is unchanged. The app surface does not expose it.)
- Markdown syntax acts only as **muscle-memory accelerators** for users who already know it: type `# ` and the heading appears immediately; the `#` collapses to a gutter marker visible only on the cursor's line. Type `**foo**` and `foo` becomes bold immediately; the `**` collapses the same way. Same for `> `, `- `, `1. `, `- [ ]`, `\`\`\``, `---`, `[text](url)`.
- Users who *don't* know markdown never need to learn it. They get heading via the slash menu (`/` → Heading) or the floating toolbar; they get bold via `Cmd-B` or the toolbar; they never type `**`.

### **Slash menu for block insertion — secondary, not centerpiece**

- The slash menu is a *discovery aid* for users who don't know the markdown shortcuts. Power users live in shortcuts; new users live in `/`.
- Type `/` at the start of an empty line → popover lists: Heading 1/2/3 · Bullet list · Numbered list · To-do · Quote · Code block · Divider · Table · Image · Embed file from Drive · Link to note · Link.
- Arrow keys + Enter to pick. `Esc` closes.
- Slash menu does **not** appear automatically as an onboarding interrupt. The empty-note placeholder mentions it once ("Press `/` for blocks, or just start typing"); after that it's invisible until invoked.
- Same posture as Obsidian's command palette + Apple Notes' format menu — *available, never insistent*.

### **`@` for people, `[[` *or* `+` for note links**

- `@` opens a workspace-member picker (inserts a mention; no notification in v0.3 — that needs the email/notifications brief).
- `+` opens a note picker (search the workspace's notes). Inserts a link; the picker has a "Create new page «typed»" footer.
- `[[` also opens the same note picker for muscle-memory parity with the current users. Both gestures resolve to the same link block.

### **Floating formatting toolbar on selection**

- Select text → small popover above the selection with: Bold · Italic · Strike · Inline code · Link · (heading 1/2/3 if the selection is at the start of a line) · Turn into → quote / list / code.
- Disappears on click-out, on `Esc`, or after 5 seconds of mouse idle.
- Mirrors what Notion / Craft / Linear all do.

### **Drag handle on every block — opt-in, hover-revealed**

- Drag handles are a Notion-shaped affordance. Useful, but not what Obsidian or macOS Notes lead with.
- We ship them, but as a quiet hover affordance: hover the left margin of a block → a 6-dot handle fades in. Hover off, it fades out.
- Click → opens a block menu (Duplicate · Move up / down · Turn into → · Delete).
- Drag → drops the block at the cursor target. Drop indicator is a 2 px accent line.
- Disabled on mobile (touch users get a long-press-to-pick-up gesture instead).
- Never visible by default; never blocks the eye while reading.

### **Mobile is a separate first-class surface**

- The desktop editor + drag handles + floating toolbar don't translate to mobile.
- Mobile gets a sticky bottom toolbar (always visible above the keyboard) with the most-used actions: bold / italic / list / heading / link / `/` opens the slash menu as a sheet.
- Long-press on a block → bottom sheet with the block menu.
- The same gestures every iOS / Android user already knows (selection handles, contextual sheets).

### **Autosave is the only save**

- `Cmd-S` keeps working but no longer shows "Saved 2 s ago" in response — it's just a no-op for muscle memory.
- The footer state becomes a single subtle dot: filled accent when there are unsaved keystrokes, hollow when caught up. No words.
- Conflict (last-write-wins) toast keeps the existing copy but pivots to a one-click "Restore your version" that diffs against the server copy.

## Locked-out decisions

- **Source / preview split, or a source toggle anywhere in the UI.** The whole point of this brief. Markdown is the storage format, never the user-facing surface. Engineers who want to see the source can read it from disk or the bucket.
- **Notion-style mandatory block UI.** Drag handles are *available* on hover but never the first thing a user sees. Slash menu is *available* but never auto-opens.
- **Lexical (Meta's editor).** Same family as Tiptap; Tiptap's input-rules + markdown-serialization combo is the better fit for live-render markdown.
- **Custom contenteditable.** Re-implementing block editing is a 6-month detour. Not worth the bus factor.
- **JSON / proprietary storage of the editor tree.** Loses the "export = `cat` it" guarantee. Markdown stays canonical.
- **Real-time collab (CRDT) in this brief.** Separate brief; needs its own design pass. The Tiptap pivot *makes* `Yjs` collab achievable later, doesn't ship it.
- **AI features (`/ask AI`, auto-summarise, translate).** Path-only — the slash menu's command list is the integration seam when AI is prioritised. No design / provider pick / implementation in this brief or any other until the user explicitly green-lights it. Stays P3 in [`../../PIPELINE.md`](../../PIPELINE.md).

## Threat model

| Risk | Mitigation |
|---|---|
| **Tiptap dep adds ~120 KB; landing route slows down** | Route-split: editor is dynamic-imported only when the Notes page mounts. Files / Cmd-K / sign-in stay unchanged. |
| **Pasted-from-Word HTML smuggles XSS** | Tiptap's HTML parser strips to a known schema; we additionally DOMPurify the output before serialization. |
| **Round-trip drift (markdown → editor → markdown changes whitespace)** | Snapshot tests on the 50 most common markdown patterns. Drift in non-rendered whitespace is acceptable; semantic drift is a regression. |
| **Slash + `@` + `+` triggers conflict with passwords / IDs in code blocks** | Slash menu doesn't open inside a code-block node. `@` and `+` open only at word boundaries. All three are easy to dismiss with `Esc`. |
| **General users don't discover slash commands** | Empty-note placeholder reads "Press `/` to insert a block or just start typing." First-run tooltip on the first empty line. |

## Migration

- **Existing notes:** stored as markdown today; new editor parses markdown on open. No data migration. No downtime.
- **Existing links:** `[[Page name]]` tokens already in note bodies parse into Tiptap's link node. Backlink index is unchanged.
- **Existing keyboard shortcuts:** `Cmd-N`, `Cmd-K`, `Cmd-S` all kept. `Tab`/`Shift-Tab` change meaning (indent/outdent a list item or move between fields; no longer "indent four spaces in the source").

## Implementation surface

Estimated 4–6 sessions of work, ~1200 LOC + tests:

- `web/src/pages/Notes.tsx` — gut the source/preview split. Mount a Tiptap editor against the note body.
- `web/src/components/notes/Editor.tsx` (new) — Tiptap setup, extensions list, markdown serializer hook.
- `web/src/components/notes/SlashMenu.tsx` (new) — Radix Popover + cmdk-style list.
- `web/src/components/notes/FormattingToolbar.tsx` (new) — Radix Popover on selection change.
- `web/src/components/notes/BlockMenu.tsx` (new) — drag handle popover (desktop only).
- `web/src/components/notes/MentionPicker.tsx` (new) — `@` member picker, `+` / `[[` note picker.
- `web/src/components/notes/MobileToolbar.tsx` (new) — sticky bottom toolbar (mobile only).
- `web/src/pages/SharePreview.tsx` — keep using `marked` + `dompurify` for the public render path. No change.
- Existing `notes` API + backlinks indexer: unchanged.

## Test plan

- **Round-trip** — 50 fixture markdown documents; assert `serialize(parse(md)) === md` modulo whitespace.
- **Slash menu** — keyboard-only flow: open, narrow, pick, insert. Each block type renders correctly.
- **`@` / `+` / `[[`** — pickers open at the right trigger; selecting a member inserts a mention; selecting a note inserts a link node.
- **Drag handle** — drag a paragraph above a heading; assert document order updates.
- **Mobile** — bottom toolbar shows above the keyboard; long-press opens block sheet.
- **Migration** — load 20 existing real markdown notes; assert no content loss.
- **A11y** — every interactive element has a role + label; the editor is reachable via Tab; screen-reader announces block changes.
- **Polish bar (10 commandments from `04-polish-principles`)** — the floating toolbar honours `prefers-reduced-motion`; slash menu reaches keyboard parity; copy is sentence-case.

## Out of scope (Phase 4+)

- **Real-time collaborative editing.** Tiptap + Yjs is the path; needs its own brief tying to [`14-presence`](./14-presence.md) and the CRDT sync surface.
- **AI block actions** (`/ask AI`, summarise, translate). **Path-only, not work.** Integration seam = the slash menu's command list. No brief, no provider pick, no UI until the user prioritises it.
- **Inline file embeds** that render previews of `.xlsx` / `.docx` inline. Different brief; touches the editor SDK handoff.
- **Database / Kanban / Calendar views on top of notes.** Notion-style "everything is a database" is a different product.
- **Comments / threads on blocks.** Different brief — needs presence and notifications first.

## When to ship

Trigger: **a non-technical user on a real team opens the current Notes app and bounces.** Concretely — the first time we get feedback that says "I couldn't figure out how to make text bold" or "I didn't know what `[[` did," this brief moves from queued to in-progress.

Pre-trigger work that unblocks it: nothing. The substrate (markdown storage, backlinks, tree, search) is already there. This is purely a SPA pivot.
