// Demo-mode backend shim — no server, browser-storage backed.
//
// Compiled in when VITE_DEMO_MODE=1 (GitHub Pages build at drive.schnsrw.live).
// Metadata persists across reloads via localStorage under `cd-demo-state-v1`.
// Uploaded file blobs live in a module-scope Map (not persisted — too large
// for localStorage). Pipeline issue #12 upgrades blob persistence to IndexedDB.
//
// Sign-in accepts any non-empty username + password — there is no security
// boundary in demo mode; we just need the flow to feel real. The pre-filled
// `demo` / `demo` credentials shown on the SignIn page are just defaults.

import type { About, FileDto, FolderDto, FolderDetail, ListResp, Me } from "./client.ts";

interface DemoShare {
  id: string;
  token: string;
  url: string;
  permissions: "view";
  has_password: boolean;
  password?: string;
  expires_at: string | null;
  created_at: string;
  last_accessed_at: string | null;
  access_count: number;
  file_id: string;
}

interface DemoState {
  signedIn: boolean;
  folders: FolderDto[];
  files: FileDto[];
  shares: DemoShare[];
  events: DemoEvent[];
  notes?: DemoNote[];
  noteLinks?: DemoNoteLink[];
  nextId: number;
  username?: string;
}

interface DemoNote {
  id: string;
  workspace_id: string;
  parent_id: string | null;
  title: string;
  body: string;
  order_key: string;
  trashed_at: string | null;
  created_at: string;
  modified_at: string;
}

interface DemoNoteLink {
  note_id: string;
  target_title: string; // lowercased
  target_id: string | null;
}

interface DemoEvent {
  id: string;
  created_at: string;
  actor_id: string | null;
  actor_username: string | null;
  action: string;
  target_kind: string | null;
  target_id: string | null;
  target_name: string | null;
  ip_address: string | null;
  metadata: string | null;
}

const STATE_KEY = "cd-demo-state-v1";
const blobs: Map<string, Blob> = new Map();

// IndexedDB-backed blob storage so uploaded files survive a page
// reload. localStorage is too small + JSON-unfriendly for binary;
// IDB scales to GBs and handles Blobs natively.
const IDB_NAME = "cd-demo";
const IDB_STORE = "blobs";
let idbReady: Promise<IDBDatabase> | null = null;
function openIdb(): Promise<IDBDatabase> {
  if (idbReady) return idbReady;
  idbReady = new Promise<IDBDatabase>((resolve, reject) => {
    if (typeof indexedDB === "undefined") {
      reject(new Error("IndexedDB unavailable"));
      return;
    }
    const req = indexedDB.open(IDB_NAME, 1);
    req.onupgradeneeded = () => {
      req.result.createObjectStore(IDB_STORE);
    };
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error ?? new Error("idb open failed"));
  });
  return idbReady;
}
async function idbPutBlob(id: string, blob: Blob): Promise<void> {
  try {
    const db = await openIdb();
    await new Promise<void>((resolve, reject) => {
      const tx = db.transaction(IDB_STORE, "readwrite");
      tx.objectStore(IDB_STORE).put(blob, id);
      tx.oncomplete = () => resolve();
      tx.onerror = () => reject(tx.error);
    });
  } catch {
    /* private mode / quota — fall through, memory map still has it */
  }
}
async function idbGetBlob(id: string): Promise<Blob | null> {
  try {
    const db = await openIdb();
    return await new Promise<Blob | null>((resolve, reject) => {
      const tx = db.transaction(IDB_STORE, "readonly");
      const req = tx.objectStore(IDB_STORE).get(id);
      req.onsuccess = () => resolve((req.result as Blob | undefined) ?? null);
      req.onerror = () => reject(req.error);
    });
  } catch {
    return null;
  }
}
async function idbDeleteBlob(id: string): Promise<void> {
  try {
    const db = await openIdb();
    await new Promise<void>((resolve, reject) => {
      const tx = db.transaction(IDB_STORE, "readwrite");
      tx.objectStore(IDB_STORE).delete(id);
      tx.oncomplete = () => resolve();
      tx.onerror = () => reject(tx.error);
    });
  } catch {
    /* ignored */
  }
}

// Seeded workspaces for the demo. Kept in-memory (not persisted)
// because the demo's workspaces are just for showing off the switcher.
const demoWorkspaces: Array<{
  id: string;
  name: string;
  kind: "personal" | "team";
  owner_id: string;
  role: "owner" | "member";
  member_count: number;
  created_at: string;
}> = [
  {
    id: "wsp_personal_demo",
    name: "Personal",
    kind: "personal",
    owner_id: "demo-user",
    role: "owner",
    member_count: 1,
    created_at: "2026-06-01T00:00:00Z",
  },
  {
    id: "wsp_team_demo",
    name: "Casual Demo",
    kind: "team",
    owner_id: "demo-user",
    role: "owner",
    member_count: 1,
    created_at: "2026-06-01T00:00:00Z",
  },
];

const state: DemoState = loadState();
// Seed in-memory content for the seeded files so the Preview Modal has
// real bytes to render. Uploads add their own blobs as the user goes.
seedBlobs();
persist();

function seedBlobs() {
  blobs.set(
    "f_readme",
    new Blob(
      [
        `# Welcome to Casual Drive\n\n` +
          `This is a **demo build** running entirely in your browser — there is\n` +
          `no server. Your changes persist in localStorage and reset only when\n` +
          `you clear browser data.\n\n` +
          `## Try it\n\n` +
          `- Right-click any file for the **context menu**.\n` +
          `- Open *Q2 planning.xlsx* to see the "Open in Casual Sheets" handoff.\n` +
          `- Pick **Share…** to mint a real share link with optional password and\n` +
          `  expiry. Open the link in another tab to see the recipient page.\n` +
          `- Drag a file into the window to upload it (some extensions are\n` +
          `  blocked for safety — try a \`.sh\`).\n` +
          `- Switch to the **Activity** tab to see every action recorded.\n\n` +
          `## What works in the real build\n\n` +
          `1. Multi-backend storage (filesystem, S3, MinIO) via OpenDAL.\n` +
          `2. Argon2id passwords + tower-sessions cookies.\n` +
          `3. WOPI handoff to [Casual Sheets](https://schnsrw.live) and\n` +
          `   Casual Editor for live co-editing.\n` +
          `4. Two-origin model — app bytes and file bytes never share an\n` +
          `   origin, so a malicious upload can't talk back to the SPA.\n\n` +
          `> Self-host the real thing from\n` +
          `> [github.com/schnsrw/drive](https://github.com/schnsrw/drive).\n`,
      ],
      { type: "text/markdown" },
    ),
  );

  blobs.set(
    "f_logo",
    new Blob(
      [
        `<svg viewBox="0 0 172 172" xmlns="http://www.w3.org/2000/svg">` +
          `<defs><clipPath id="c"><rect width="172" height="172" rx="40"/></clipPath></defs>` +
          `<g clip-path="url(#c)">` +
          `<rect width="172" height="172" fill="#16161A"/>` +
          `<g fill="#F5F3EE">` +
          `<circle cx="54" cy="100" r="23"/>` +
          `<circle cx="118" cy="100" r="23"/>` +
          `<circle cx="86" cy="68" r="34"/>` +
          `<rect x="54" y="100" width="64" height="23"/>` +
          `</g></g></svg>`,
      ],
      { type: "image/svg+xml" },
    ),
  );
}

function loadState(): DemoState {
  // localStorage may throw in private-mode Safari; never let it break boot.
  try {
    const raw = typeof window !== "undefined" ? window.localStorage.getItem(STATE_KEY) : null;
    if (raw) {
      const parsed = JSON.parse(raw) as Partial<DemoState>;
      if (Array.isArray(parsed.folders) && Array.isArray(parsed.files)) {
        return {
          signedIn: parsed.signedIn ?? false,
          folders: parsed.folders,
          files: parsed.files,
          shares: Array.isArray(parsed.shares) ? parsed.shares : [],
          events:
            Array.isArray((parsed as { events?: DemoEvent[] }).events) &&
            (parsed as { events: DemoEvent[] }).events.length > 0
              ? (parsed as { events: DemoEvent[] }).events
              : seedEvents(),
          nextId: typeof parsed.nextId === "number" ? parsed.nextId : 1000,
          username: parsed.username,
        };
      }
    }
  } catch {
    // Fall through to seed.
  }
  return {
    signedIn: false,
    folders: seedFolders(),
    files: seedFiles(),
    shares: [],
    events: seedEvents(),
    nextId: 1000,
  };
}

function seedEvents(): DemoEvent[] {
  // Backdated to make the timeline read like real history when the visitor
  // opens the Activity page on first visit. Times in UTC; the SPA converts
  // them to local for display.
  const evt = (
    minutesAgo: number,
    action: string,
    extras: Partial<DemoEvent> = {},
  ): DemoEvent => ({
    id: `evt_seed_${minutesAgo}`,
    created_at: new Date(Date.now() - minutesAgo * 60_000).toISOString(),
    actor_id: "demo-user",
    actor_username: "demo",
    action,
    target_kind: null,
    target_id: null,
    target_name: null,
    ip_address: null,
    metadata: null,
    ...extras,
  });
  return [
    evt(2, "auth.sign_in", { target_kind: "session", target_id: "demo-sid" }),
    evt(8, "files.upload", {
      target_kind: "file",
      target_id: "f_brief",
      target_name: "Product brief.docx",
      metadata: '{"size":41200}',
    }),
    evt(34, "share.create", {
      target_kind: "share_link",
      target_id: "shl_demo",
      target_name: "Q2 planning.xlsx",
      metadata: '{"has_password":false}',
    }),
    evt(70, "folders.create", {
      target_kind: "folder",
      target_id: "fld_designs",
      target_name: "Design references",
    }),
    evt(165, "share.access", {
      actor_id: null,
      actor_username: null,
      target_kind: "share_link",
      target_id: "shl_demo",
      target_name: "Q2 planning.xlsx",
      metadata: '{"token":"Z3kQaB"}',
    }),
    evt(1380, "auth.sign_in_failed", {
      actor_id: null,
      actor_username: null,
      target_kind: "user",
      target_id: null,
      target_name: "owner",
    }),
    evt(1500, "files.rename", {
      target_kind: "file",
      target_id: "f_readme",
      target_name: "README.md",
    }),
  ];
}

function emitDemo(event: Omit<DemoEvent, "id" | "created_at">) {
  state.events.unshift({
    id: nextId("evt"),
    created_at: nowIso(),
    ...event,
  });
  // Cap the in-memory event log so localStorage doesn't grow without bound
  // in long-lived demo sessions.
  if (state.events.length > 500) state.events.length = 500;
  persist();
}

function persist(): void {
  try {
    window.localStorage.setItem(STATE_KEY, JSON.stringify(state));
  } catch {
    // Quota exhausted / private mode — silently degrade to ephemeral.
  }
}

function nextId(prefix: string): string {
  state.nextId += 1;
  return `${prefix}_${state.nextId.toString(36)}`;
}

function nowIso(): string {
  return new Date().toISOString();
}

function seedFolders(): FolderDto[] {
  const base = "2026-05-22T10:00:00Z";
  return [
    { id: "fld_projects", parent_id: null, name: "Projects", created_at: base, modified_at: base },
    { id: "fld_designs", parent_id: null, name: "Design references", created_at: base, modified_at: base },
    { id: "fld_personal", parent_id: null, name: "Personal", created_at: base, modified_at: base },
  ];
}

function seedFiles(): FileDto[] {
  const t = (d: string) => `2026-${d}T15:30:00Z`;
  return [
    {
      id: "f_quarter",
      parent_id: null,
      name: "Q2 planning.xlsx",
      size: 28_400,
      content_type: "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
      version: 3,
      created_at: t("05-10"),
      modified_at: t("06-04"),
    },
    {
      id: "f_brief",
      parent_id: null,
      name: "Product brief.docx",
      size: 41_200,
      content_type: "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
      version: 5,
      created_at: t("05-12"),
      modified_at: t("06-03"),
    },
    {
      id: "f_arch",
      parent_id: null,
      name: "Architecture.pdf",
      size: 1_184_000,
      content_type: "application/pdf",
      version: 1,
      created_at: t("05-15"),
      modified_at: t("05-29"),
    },
    {
      id: "f_logo",
      parent_id: null,
      name: "Logo mark.svg",
      size: 4_300,
      content_type: "image/svg+xml",
      version: 1,
      created_at: t("05-18"),
      modified_at: t("05-18"),
    },
    {
      id: "f_demo",
      parent_id: null,
      name: "Demo walkthrough.mp4",
      size: 18_400_000,
      content_type: "video/mp4",
      version: 1,
      created_at: t("05-20"),
      modified_at: t("05-20"),
    },
    {
      id: "f_readme",
      parent_id: null,
      name: "README.md",
      size: 2_100,
      content_type: "text/markdown",
      version: 2,
      created_at: t("05-22"),
      modified_at: t("06-01"),
    },
  ];
}

function listChildren(parentId: string | null): ListResp {
  return {
    folders: state.folders.filter((f) => f.parent_id === parentId),
    files: state.files.filter((f) => f.parent_id === parentId),
  };
}

export async function demoRequest<T>(path: string, init: RequestInit & { json?: unknown } = {}): Promise<T> {
  // Light latency so the UI's loading/transition states are visible — feels
  // more like a real product, not a static fixture.
  await new Promise((r) => setTimeout(r, 90 + Math.floor(Math.random() * 60)));

  const method = (init.method ?? "GET").toUpperCase();
  const url = new URL(path, "http://demo.local");
  const p = url.pathname;

  // ─── Setup ───────────────────────────────────────────────────────────
  if (p === "/api/setup/status" && method === "GET") {
    // The demo seeds a workspace owner, so the wizard never fires.
    return { needs_setup: false } as unknown as T;
  }
  if (p === "/api/setup/admin" && method === "POST") {
    throw makeError(409, "setup already complete");
  }

  // ─── Auth ────────────────────────────────────────────────────────────
  if (p === "/api/auth/sign-in" && method === "POST") {
    const body = init.json as { username?: string; password?: string };
    state.signedIn = true;
    state.username = body?.username?.trim() || "demo";
    emitDemo({
      actor_id: "demo-user",
      actor_username: state.username,
      action: "auth.sign_in",
      target_kind: "session",
      target_id: "demo-sid",
      target_name: null,
      ip_address: null,
      metadata: null,
    });
    return { csrf_token: "demo-csrf" } as unknown as T;
  }
  if (p === "/api/auth/change-password" && method === "POST") {
    const body = init.json as { old_password: string; new_password: string };
    if (!body?.new_password || body.new_password.length < 12) {
      throw makeError(422, "new password must be at least 12 characters");
    }
    if (body.new_password === body.old_password) {
      throw makeError(422, "new password must differ from the old one");
    }
    return undefined as T;
  }
  if (p === "/api/about" && method === "GET") {
    if (!state.signedIn) throw makeError(401, "not signed in");
    return {
      version: "0.0.1 (demo)",
      git_sha: "demo",
      built_at: new Date().toISOString(),
      license: "Apache-2.0",
      repository: "https://github.com/schnsrw/drive",
      storage_backend: "Browser (localStorage)",
      db_backend: "Browser (localStorage)",
      signed_url_ttl_secs: 300,
      body_limit_mb: 100,
    } satisfies About as unknown as T;
  }
  if (p === "/api/auth/sign-out" && method === "POST") {
    emitDemo({
      actor_id: "demo-user",
      actor_username: state.username ?? "demo",
      action: "auth.sign_out",
      target_kind: "session",
      target_id: "demo-sid",
      target_name: null,
      ip_address: null,
      metadata: null,
    });
    state.signedIn = false;
    persist();
    return undefined as T;
  }
  if (p === "/api/search" && method === "GET") {
    if (!state.signedIn) throw makeError(401, "not signed in");
    const q = url.searchParams.get("q")?.trim().toLowerCase() ?? "";
    const limit = Math.max(
      1,
      Math.min(200, Number.parseInt(url.searchParams.get("limit") ?? "30", 10) || 30),
    );
    // Empty-query empty-filters → return empty result. Anything else
    // runs the substring match. Phase 3 wire shape: include `notes`,
    // `total`, `next_cursor` (null in demo — no pagination), and
    // `sort_applied` so the SPA's new state machine doesn't crash on
    // undefined fields.
    if (!q) {
      return {
        files: [],
        folders: [],
        notes: [],
        total: { files: 0, folders: 0, notes: 0, exact: true },
        next_cursor: null,
        sort_applied: url.searchParams.get("sort") ?? "modified",
      } as unknown as T;
    }
    const matchedFolders = state.folders
      .filter((f) => f.name.toLowerCase().includes(q))
      .slice(0, limit);
    const matchedFiles = state.files
      .filter((f) => f.name.toLowerCase().includes(q))
      .slice(0, limit);
    return {
      files: matchedFiles,
      folders: matchedFolders,
      notes: [],
      total: {
        files: matchedFiles.length,
        folders: matchedFolders.length,
        notes: 0,
        exact: true,
      },
      next_cursor: null,
      sort_applied: url.searchParams.get("sort") ?? "modified",
    } as unknown as T;
  }
  // Workspaces — demo has Personal + one seeded Team workspace ("Demo")
  // with the demo user as Owner of both. Create/rename/transfer are
  // shimmed in-memory.
  if (p === "/api/workspaces" && method === "GET") {
    if (!state.signedIn) throw makeError(401, "not signed in");
    return {
      current_id: demoWorkspaces[0]?.id ?? "",
      workspaces: demoWorkspaces,
    } as unknown as T;
  }
  if (p === "/api/workspaces" && method === "POST") {
    if (!state.signedIn) throw makeError(401, "not signed in");
    const body = init.json as { name?: string };
    const name = (body?.name ?? "").trim();
    if (name.length < 2) throw makeError(400, "workspace name must be 2–60 characters");
    const w = {
      id: nextId("ws"),
      name,
      kind: "team" as const,
      owner_id: "demo-user",
      role: "owner" as const,
      member_count: 1,
      created_at: nowIso(),
    };
    demoWorkspaces.push(w);
    return w as unknown as T;
  }
  // Admin user management — demo just acks the calls so the UI is
  // navigable. No second user actually exists.
  if (p === "/api/admin/users" && method === "GET") {
    if (!state.signedIn) throw makeError(401, "not signed in");
    return {
      users: [
        {
          id: "demo-user",
          username: state.username ?? "demo",
          is_admin: true,
          created_at: "2026-06-01T00:00:00Z",
          used_bytes: state.files.reduce((acc, f) => acc + (f.size ?? 0), 0),
          quota_bytes: null,
        },
      ],
    } as unknown as T;
  }
  if (p === "/api/admin/users" && method === "POST") {
    if (!state.signedIn) throw makeError(401, "not signed in");
    return {
      id: "demo-user-2",
      username: ((init.json as { username?: string })?.username ?? "alice").trim(),
      is_admin: false,
      created_at: nowIso(),
      used_bytes: 0,
      quota_bytes: (init.json as { quota_bytes?: number | null })?.quota_bytes ?? null,
    } as unknown as T;
  }
  const setQuotaMatch = p.match(/^\/api\/admin\/users\/([^/]+)\/quota$/);
  if (setQuotaMatch && method === "PATCH") {
    if (!state.signedIn) throw makeError(401, "not signed in");
    return undefined as T;
  }
  if (p === "/api/me/quota/request" && method === "POST") {
    if (!state.signedIn) throw makeError(401, "not signed in");
    emitDemo({
      actor_id: "demo-user",
      actor_username: state.username ?? "demo",
      action: "quota.upgrade_request",
      target_kind: "user",
      target_id: "demo-user",
      target_name: state.username ?? "demo",
      ip_address: null,
      metadata: init.json ? JSON.stringify(init.json) : null,
    });
    return undefined as T;
  }

  if (p === "/api/admin/system" && method === "GET") {
    if (!state.signedIn) throw makeError(401, "not signed in");
    const recent = state.events
      .filter((e) => e.action === "auth.sign_in" || e.action === "auth.sign_in_failed")
      .slice(0, 10)
      .map((e) => ({
        actor_username: e.actor_username ?? e.target_name,
        ok: e.action === "auth.sign_in",
        at: e.created_at,
      }));
    return {
      version: "0.0.1 (demo)",
      git_sha: "demo",
      built_at: new Date().toISOString(),
      license: "Apache-2.0",
      storage_backend: "Browser (localStorage)",
      storage_config: { fs_root: null, s3_bucket: null, s3_endpoint: null, s3_region: null },
      db_backend: "Browser (localStorage)",
      uptime_seconds: Math.floor(performance.now() / 1000),
      active_sessions: state.signedIn ? 1 : 0,
      healthy: true,
      recent_sign_ins: recent,
    } as unknown as T;
  }
  if (p === "/api/activity" && method === "GET") {
    if (!state.signedIn) throw makeError(401, "not signed in");
    const before = url.searchParams.get("before");
    const limit = Math.max(
      1,
      Math.min(200, Number.parseInt(url.searchParams.get("limit") ?? "50", 10) || 50),
    );
    const filtered = before
      ? state.events.filter((e) => e.created_at < before)
      : state.events.slice();
    const page = filtered.slice(0, limit);
    const next_before = page.length === limit && filtered.length > limit ? page[page.length - 1].created_at : null;
    return { events: page, next_before } as unknown as T;
  }
  if (p === "/api/me" && method === "GET") {
    if (!state.signedIn) throw makeError(401, "not signed in");
    const used = state.files.reduce((acc, f) => acc + (f.size ?? 0), 0);
    return {
      admin: state.username ?? "demo",
      backend: "Browser (localStorage)",
      user_id: "demo-user",
      is_admin: true,
      used_bytes: used,
      quota_bytes: null,
    } satisfies Me as unknown as T;
  }

  // ─── Folders ─────────────────────────────────────────────────────────
  if (p === "/api/folders/root/children" && method === "GET") {
    return listChildren(null) as unknown as T;
  }
  const folderMatch = p.match(/^\/api\/folders\/([^/]+)$/);
  if (folderMatch) {
    const fid = decodeURIComponent(folderMatch[1]);
    const idx = state.folders.findIndex((f) => f.id === fid);
    if (idx === -1) throw makeError(404, "folder not found");
    if (method === "GET") {
      return { folder: state.folders[idx], children: listChildren(fid) } satisfies FolderDetail as unknown as T;
    }
    if (method === "PATCH") {
      const body = init.json as { name?: string; parent_id?: string | null };
      const updated: FolderDto = {
        ...state.folders[idx],
        name: body.name ?? state.folders[idx].name,
        parent_id: body.parent_id ?? state.folders[idx].parent_id,
        modified_at: nowIso(),
      };
      state.folders[idx] = updated;
      persist();
      return updated as unknown as T;
    }
  }
  if (p === "/api/folders" && method === "POST") {
    const body = init.json as { name: string; parent_id: string | null };
    const f: FolderDto = {
      id: nextId("fld"),
      parent_id: body.parent_id ?? null,
      name: body.name,
      created_at: nowIso(),
      modified_at: nowIso(),
    };
    state.folders.push(f);
    emitDemo({
      actor_id: "demo-user",
      actor_username: state.username ?? "demo",
      action: "folders.create",
      target_kind: "folder",
      target_id: f.id,
      target_name: f.name,
      ip_address: null,
      metadata: null,
    });
    persist();
    return f as unknown as T;
  }

  // ─── Files ───────────────────────────────────────────────────────────
  if (p === "/api/files" && method === "POST") {
    const fd = init.body as FormData;
    const file = fd.get("file") as File;
    const parentId = (fd.get("parent_id") as string | null) ?? null;
    const thumb = (fd.get("thumbnail") as string | null) ?? null;
    const fileDto: FileDto = {
      id: nextId("f"),
      parent_id: parentId,
      name: file.name,
      size: file.size,
      content_type: file.type || null,
      version: 1,
      created_at: nowIso(),
      modified_at: nowIso(),
      thumbnail: thumb && thumb.startsWith("data:image/") ? thumb : null,
    };
    blobs.set(fileDto.id, file);
    void idbPutBlob(fileDto.id, file);
    state.files.push(fileDto);
    emitDemo({
      actor_id: "demo-user",
      actor_username: state.username ?? "demo",
      action: "files.upload",
      target_kind: "file",
      target_id: fileDto.id,
      target_name: fileDto.name,
      ip_address: null,
      metadata: JSON.stringify({ size: fileDto.size }),
    });
    persist();
    return fileDto as unknown as T;
  }
  // Demo doesn't ship with a live Casual Sheets / Editor — pretend the
  // origin is unconfigured. The SPA shows a polished "self-host to use
  // this" toast and falls back to a Download action.
  const openMatch = p.match(/^\/api\/files\/([^/]+)\/open$/);
  if (openMatch && method === "GET") {
    if (!state.signedIn) throw makeError(401, "not signed in");
    throw makeError(503, "editor not configured");
  }

  // SDK content endpoints — `GET /api/files/{id}/content` returns the
  // raw bytes the editor renders against; `PUT` replaces them.
  // The demo doesn't persist real bytes for its seeded files, so GET
  // returns an empty buffer (the editor mounts in an "empty document"
  // state) and PUT is a no-op success. This is what keeps the SDK
  // editor functional inside the demo without 404 noise in console.
  const contentMatch = p.match(/^\/api\/files\/([^/]+)\/content$/);
  if (contentMatch) {
    if (!state.signedIn) throw makeError(401, "not signed in");
    const fid = decodeURIComponent(contentMatch[1]);
    const file = state.files.find((f) => f.id === fid);
    if (!file) throw makeError(404, "file not found");
    if (method === "GET") {
      // Resolve bytes in priority: in-memory blobs Map → IndexedDB
      // (survives reloads) → empty Blob (seeded files that ship
      // metadata-only). The editor renders its parse-error UI cleanly
      // on the empty fallback. `as T` because the shim's return type
      // is generic by route.
      const stored = blobs.get(fid) ?? (await idbGetBlob(fid)) ?? null;
      if (stored) blobs.set(fid, stored); // warm the memory cache
      const body =
        stored ??
        new Blob([], { type: file.content_type ?? "application/octet-stream" });
      return (new Response(body, {
        status: 200,
        headers: { "Content-Type": file.content_type ?? "application/octet-stream" },
      }) as unknown) as T;
    }
    if (method === "PUT") {
      // Persist the new bytes to both in-memory blobs and IDB so a
      // reload after an edit doesn't undo the save. Size + version
      // bump so the autosave chrome shows "saved 1s ago".
      const bodyBlob =
        init.body instanceof Blob
          ? init.body
          : init.body instanceof ArrayBuffer
            ? new Blob([init.body], {
                type: file.content_type ?? "application/octet-stream",
              })
            : null;
      if (bodyBlob) {
        blobs.set(fid, bodyBlob);
        void idbPutBlob(fid, bodyBlob);
      }
      const size = init.body instanceof Blob
        ? init.body.size
        : init.body instanceof ArrayBuffer
          ? init.body.byteLength
          : 0;
      const idx = state.files.findIndex((f) => f.id === fid);
      if (idx >= 0) {
        state.files[idx] = {
          ...state.files[idx],
          size,
          modified_at: nowIso(),
          version: state.files[idx].version + 1,
        };
        persist();
      }
      return undefined as T;
    }
  }

  const fileMatch = p.match(/^\/api\/files\/([^/]+)(\/(trash|download))?$/);
  if (fileMatch) {
    const fid = decodeURIComponent(fileMatch[1]);
    const sub = fileMatch[3];
    const idx = state.files.findIndex((f) => f.id === fid);
    if (idx === -1) throw makeError(404, "file not found");
    // GET /api/files/{id} — metadata for the cold `/file/<id>` load.
    if (method === "GET" && !sub) {
      return state.files[idx] as unknown as T;
    }
    if (method === "PATCH" && !sub) {
      const body = init.json as { name?: string; parent_id?: string | null };
      const next: FileDto = {
        ...state.files[idx],
        name: body.name ?? state.files[idx].name,
        parent_id: body.parent_id ?? state.files[idx].parent_id,
        modified_at: nowIso(),
        version: state.files[idx].version + 1,
      };
      state.files[idx] = next;
      if (body.name) {
        emitDemo({
          actor_id: "demo-user",
          actor_username: state.username ?? "demo",
          action: "files.rename",
          target_kind: "file",
          target_id: next.id,
          target_name: next.name,
          ip_address: null,
          metadata: null,
        });
      }
      persist();
      return next as unknown as T;
    }
    if (method === "POST" && sub === "trash") {
      const f = state.files[idx];
      state.files.splice(idx, 1);
      blobs.delete(fid);
      void idbDeleteBlob(fid);
      emitDemo({
        actor_id: "demo-user",
        actor_username: state.username ?? "demo",
        action: "files.trash",
        target_kind: "file",
        target_id: f.id,
        target_name: f.name,
        ip_address: null,
        metadata: null,
      });
      persist();
      return undefined as T;
    }
  }

  // ─── Sharing ─────────────────────────────────────────────────────────
  const shareCreateMatch = p.match(/^\/api\/files\/([^/]+)\/share$/);
  if (shareCreateMatch && method === "POST") {
    if (!state.signedIn) throw makeError(401, "not signed in");
    const fid = decodeURIComponent(shareCreateMatch[1]);
    const file = state.files.find((f) => f.id === fid);
    if (!file) throw makeError(404, "file not found");
    const body = init.json as {
      permissions?: string;
      password?: string | null;
      expires_in_seconds?: number | null;
    };
    if (body.permissions && body.permissions !== "view") {
      throw makeError(400, "only 'view' permissions ship in v0");
    }
    const token = randomToken();
    const created_at = nowIso();
    const expires_at =
      body.expires_in_seconds && body.expires_in_seconds > 0
        ? new Date(Date.now() + body.expires_in_seconds * 1000).toISOString()
        : null;
    const share: DemoShare = {
      id: nextId("shl"),
      token,
      url: `${window.location.origin}/s/${token}`,
      permissions: "view",
      has_password: !!(body.password && body.password.trim()),
      password: body.password?.trim() || undefined,
      expires_at,
      created_at,
      last_accessed_at: null,
      access_count: 0,
      file_id: fid,
    };
    state.shares.unshift(share);
    emitDemo({
      actor_id: "demo-user",
      actor_username: state.username ?? "demo",
      action: "share.create",
      target_kind: "share_link",
      target_id: share.id,
      target_name: file.name,
      ip_address: null,
      metadata: JSON.stringify({ file_id: fid, has_password: share.has_password }),
    });
    persist();
    const { password: _pwd, file_id: _fid, ...dto } = share;
    void _pwd;
    void _fid;
    return dto as unknown as T;
  }
  const shareListMatch = p.match(/^\/api\/files\/([^/]+)\/shares$/);
  if (shareListMatch && method === "GET") {
    if (!state.signedIn) throw makeError(401, "not signed in");
    const fid = decodeURIComponent(shareListMatch[1]);
    const shares = state.shares
      .filter((s) => s.file_id === fid)
      .map(({ password: _p, file_id: _f, ...rest }) => {
        void _p;
        void _f;
        return rest;
      });
    return { shares } as unknown as T;
  }
  const shareRevokeMatch = p.match(/^\/api\/shares\/([^/]+)$/);
  if (shareRevokeMatch && method === "DELETE") {
    if (!state.signedIn) throw makeError(401, "not signed in");
    const sid = decodeURIComponent(shareRevokeMatch[1]);
    const revoked = state.shares.find((s) => s.id === sid);
    if (!revoked) throw makeError(404, "not found");
    state.shares = state.shares.filter((s) => s.id !== sid);
    const fileName = state.files.find((f) => f.id === revoked.file_id)?.name ?? null;
    emitDemo({
      actor_id: "demo-user",
      actor_username: state.username ?? "demo",
      action: "share.revoke",
      target_kind: "share_link",
      target_id: revoked.id,
      target_name: fileName,
      ip_address: null,
      metadata: null,
    });
    persist();
    return undefined as T;
  }
  const shareResolveMatch = p.match(/^\/api\/share\/([^/]+)$/);
  if (shareResolveMatch && method === "POST") {
    const token = decodeURIComponent(shareResolveMatch[1]);
    const share = state.shares.find((s) => s.token === token);
    if (!share) throw makeError(404, "not found");
    if (share.expires_at && new Date(share.expires_at) < new Date()) {
      throw makeError(410, "expired");
    }
    if (share.has_password) {
      const body = init.json as { password?: string | null };
      const candidate = body?.password ?? "";
      if (!candidate || candidate !== share.password) {
        throw makeError(401, "password required");
      }
    }
    const file = state.files.find((f) => f.id === share.file_id);
    if (!file) throw makeError(404, "file gone");
    share.access_count += 1;
    share.last_accessed_at = nowIso();
    emitDemo({
      actor_id: null,
      actor_username: null,
      action: "share.access",
      target_kind: "share_link",
      target_id: share.id,
      target_name: file.name,
      ip_address: null,
      metadata: JSON.stringify({ token: share.token }),
    });
    persist();
    return {
      file: {
        name: file.name,
        size: file.size,
        content_type: file.content_type,
        modified_at: file.modified_at,
      },
      download_url: `/api/share/${token}/download`,
      permissions: share.permissions,
    } as unknown as T;
  }

  // ── Notes / Wiki — pipeline §8.11 ────────────────────────────────
  // Minimal in-memory shim so the demo /notes surface works end-to-end
  // without a server. State is namespaced under state.notes / noteLinks
  // and persists with everything else via persist().
  {
    const notes = (state.notes ??= []);
    const links = (state.noteLinks ??= []);
    const activeWsCandidate =
      (typeof window !== "undefined"
        ? window.localStorage.getItem("cd-workspace-id-v1")
        : null) || demoWorkspaces[0].id;

    function indexLinks(noteId: string, body: string, workspaceId: string) {
      const set = new Set<string>();
      const re = /\[\[([^\]\n]+)\]\]/g;
      let m: RegExpExecArray | null;
      while ((m = re.exec(body)) !== null) {
        const t = m[1].trim().toLowerCase();
        if (t) set.add(t);
      }
      const filtered = links.filter((l) => l.note_id !== noteId);
      links.length = 0;
      links.push(...filtered);
      const byTitle = new Map<string, string>();
      for (const n of notes) {
        if (n.workspace_id === workspaceId && !n.trashed_at) {
          byTitle.set(n.title.toLowerCase(), n.id);
        }
      }
      for (const t of set) {
        links.push({ note_id: noteId, target_title: t, target_id: byTitle.get(t) ?? null });
      }
    }

    function backlinksFor(noteId: string, title: string): { id: string; title: string }[] {
      const titleLower = title.toLowerCase();
      const ids = new Set<string>();
      for (const l of links) {
        if (l.note_id === noteId) continue;
        if (l.target_id === noteId || l.target_title === titleLower) {
          ids.add(l.note_id);
        }
      }
      const out: { id: string; title: string }[] = [];
      for (const id of ids) {
        const n = notes.find((x) => x.id === id && !x.trashed_at);
        if (n) out.push({ id: n.id, title: n.title });
      }
      return out.slice(0, 50);
    }

    function toDto(n: DemoNote): Record<string, unknown> {
      return {
        id: n.id,
        workspace_id: n.workspace_id,
        parent_id: n.parent_id,
        title: n.title,
        body: n.body,
        order_key: n.order_key,
        created_at: n.created_at,
        modified_at: n.modified_at,
        backlinks: backlinksFor(n.id, n.title),
      };
    }

    function nodeDto(n: DemoNote) {
      return { id: n.id, parent_id: n.parent_id, title: n.title, order_key: n.order_key };
    }

    if (p === "/api/notes/tree" && method === "GET") {
      const ws = url.searchParams.get("workspace") ?? activeWsCandidate;
      const live = notes
        .filter((n) => n.workspace_id === ws && !n.trashed_at)
        .sort((a, b) => a.order_key.localeCompare(b.order_key));
      const trashed = notes
        .filter((n) => n.workspace_id === ws && n.trashed_at)
        .sort((a, b) => (b.trashed_at ?? "").localeCompare(a.trashed_at ?? ""));
      return {
        workspace_id: ws,
        nodes: live.map(nodeDto),
        trashed: trashed.map(nodeDto),
      } as unknown as T;
    }

    if (p === "/api/notes/search" && method === "GET") {
      const q = (url.searchParams.get("q") ?? "").toLowerCase();
      if (!q) return [] as unknown as T;
      const ws = url.searchParams.get("workspace") ?? activeWsCandidate;
      const hits = notes
        .filter(
          (n) =>
            n.workspace_id === ws &&
            !n.trashed_at &&
            (n.title.toLowerCase().includes(q) || n.body.toLowerCase().includes(q)),
        )
        .slice(0, 50)
        .map(nodeDto);
      return hits as unknown as T;
    }

    if (p === "/api/notes" && method === "POST") {
      const body = init.json as {
        workspace_id?: string;
        parent_id?: string | null;
        title: string;
      };
      const ws = body.workspace_id ?? activeWsCandidate;
      const title = (body.title ?? "").trim() || "Untitled";
      const now = nowIso();
      const note: DemoNote = {
        id: `note_${++state.nextId}`,
        workspace_id: ws,
        parent_id: body.parent_id ?? null,
        title,
        body: "",
        order_key: `m${state.nextId.toString(36)}`,
        trashed_at: null,
        created_at: now,
        modified_at: now,
      };
      notes.push(note);
      // Resolve any dangling links to the new title.
      const titleLower = title.toLowerCase();
      for (const l of links) {
        if (l.target_id === null && l.target_title === titleLower) {
          l.target_id = note.id;
        }
      }
      emitDemo({
        actor_id: null,
        actor_username: state.username ?? "demo",
        action: "notes.create",
        target_kind: "note",
        target_id: note.id,
        target_name: note.title,
        ip_address: null,
        metadata: null,
      });
      persist();
      return toDto(note) as unknown as T;
    }

    const noteMatch = /^\/api\/notes\/([^/]+)(?:\/(trash|restore))?$/.exec(p);
    if (noteMatch) {
      const id = noteMatch[1];
      const action = noteMatch[2];
      const n = notes.find((x) => x.id === id);
      if (!n) throw makeError(404, "note not found");

      if (!action && method === "GET") {
        return toDto(n) as unknown as T;
      }
      if (!action && method === "PATCH") {
        const body = init.json as {
          title?: string;
          body?: string;
          parent_id?: string | null;
          order_key?: string;
        };
        if (body.title !== undefined) n.title = body.title.trim() || n.title;
        if (body.parent_id !== undefined) n.parent_id = body.parent_id ?? null;
        if (body.order_key !== undefined) n.order_key = body.order_key;
        if (body.body !== undefined) {
          if (body.body.length > 1_048_576) throw makeError(413, "note body too large");
          n.body = body.body;
          indexLinks(n.id, n.body, n.workspace_id);
        }
        n.modified_at = nowIso();
        emitDemo({
          actor_id: null,
          actor_username: state.username ?? "demo",
          action: body.body !== undefined ? "notes.edit" : "notes.update",
          target_kind: "note",
          target_id: n.id,
          target_name: n.title,
          ip_address: null,
          metadata: null,
        });
        persist();
        return toDto(n) as unknown as T;
      }
      if (!action && method === "DELETE") {
        const idx = notes.findIndex((x) => x.id === id);
        if (idx !== -1) notes.splice(idx, 1);
        for (let i = links.length - 1; i >= 0; i--) {
          if (links[i].note_id === id) links.splice(i, 1);
        }
        persist();
        return undefined as unknown as T;
      }
      if (action === "trash" && method === "POST") {
        n.trashed_at = nowIso();
        n.modified_at = n.trashed_at;
        persist();
        return undefined as unknown as T;
      }
      if (action === "restore" && method === "POST") {
        n.trashed_at = null;
        n.modified_at = nowIso();
        persist();
        return undefined as unknown as T;
      }
    }
  }

  throw makeError(501, `demo: route not implemented (${method} ${p})`);
}

function randomToken(): string {
  // 16 bytes of randomness → URL-safe base64 of length 22 (no padding).
  // Uses crypto.getRandomValues so the demo at least *looks* legit; we
  // never check this token against a server.
  const bytes = new Uint8Array(16);
  (crypto as Crypto).getRandomValues(bytes);
  let bin = "";
  for (const b of bytes) bin += String.fromCharCode(b);
  return btoa(bin).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

export function demoDownloadUrl(fileId: string): string {
  const file = state.files.find((f) => f.id === fileId);
  const blob = blobs.get(fileId);
  if (blob) return URL.createObjectURL(blob);
  // Seeded files have no blob (and uploads don't survive a reload) —
  // synthesize a tiny placeholder so the browser actually downloads
  // something the user can open.
  const placeholder = new Blob(
    [`Casual Drive demo · ${file?.name ?? fileId}\n\nThis is placeholder content. The live build serves real bytes.\n`],
    { type: "text/plain" },
  );
  return URL.createObjectURL(placeholder);
}

/** Share-link download in demo mode — synthesize a placeholder for the
 * underlying file. Used by Recipient when window.location.assign'd. */
export function demoShareDownload(token: string): string | null {
  const share = state.shares.find((s) => s.token === token);
  if (!share) return null;
  return demoDownloadUrl(share.file_id);
}

/** Hard-reset the demo. Wipes everything in localStorage and reloads.
 * Exposed on window for ad-hoc debugging (`__cdResetDemo()` in DevTools). */
export function resetDemo(): void {
  try {
    window.localStorage.removeItem(STATE_KEY);
  } catch {
    /* ignored */
  }
  window.location.reload();
}

if (typeof window !== "undefined") {
  (window as unknown as { __cdResetDemo?: () => void }).__cdResetDemo = resetDemo;
}

function makeError(status: number, message: string) {
  const err = new Error(message) as Error & { status: number; body: unknown };
  err.status = status;
  err.body = { error: message };
  return err;
}
