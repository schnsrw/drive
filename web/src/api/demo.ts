// Demo-mode backend shim — no server, in-memory only.
//
// Compiled in when VITE_DEMO_MODE=1 (GitHub Pages build at drive.schnsrw.live).
// State lives in module-scope arrays and is lost on reload. Pipeline issue #12
// will swap this for IndexedDB so demo data survives across visits.
//
// Sign-in accepts any non-empty username + password — there is no security
// boundary in demo mode; we just need the flow to feel real.

import type { FileDto, FolderDto, FolderDetail, ListResp, Me, SignInResp } from "./client.ts";

interface DemoFile extends FileDto {
  blob?: Blob;
}

let signedIn = false;
const folders: FolderDto[] = seedFolders();
const files: DemoFile[] = seedFiles();
let nextId = 1000;

function id(prefix: string): string {
  nextId += 1;
  return `${prefix}_${nextId.toString(36)}`;
}

function nowIso(): string {
  // Same fixed reference time as the seed so the relative-time ordering
  // looks reasonable on first paint. Replaced live as the user edits.
  const t = new Date();
  return t.toISOString();
}

function seedFolders(): FolderDto[] {
  const base = "2026-05-22T10:00:00Z";
  return [
    { id: "fld_projects", parent_id: null, name: "Projects", created_at: base, modified_at: base },
    { id: "fld_designs", parent_id: null, name: "Design references", created_at: base, modified_at: base },
    { id: "fld_personal", parent_id: null, name: "Personal", created_at: base, modified_at: base },
  ];
}

function seedFiles(): DemoFile[] {
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
    folders: folders.filter((f) => f.parent_id === parentId),
    files: files
      .filter((f) => f.parent_id === parentId)
      .map(({ blob, ...rest }) => {
        void blob;
        return rest;
      }),
  };
}

function jsonResp<T>(body: T): T {
  return body;
}

export async function demoRequest<T>(path: string, init: RequestInit & { json?: unknown } = {}): Promise<T> {
  // Light latency so the UI's loading/transition states are visible — feels
  // more like a real product, not a static fixture.
  await new Promise((r) => setTimeout(r, 90 + Math.floor(Math.random() * 60)));

  const method = (init.method ?? "GET").toUpperCase();
  const url = new URL(path, "http://demo.local");
  const p = url.pathname;

  // ─── Auth ────────────────────────────────────────────────────────────
  if (p === "/api/auth/sign-in" && method === "POST") {
    signedIn = true;
    return jsonResp<SignInResp>({ csrf_token: "demo-csrf" }) as unknown as T;
  }
  if (p === "/api/auth/sign-out" && method === "POST") {
    signedIn = false;
    return undefined as T;
  }
  if (p === "/api/me" && method === "GET") {
    if (!signedIn) throw makeError(401, "not signed in");
    return jsonResp<Me>({
      admin: "demo",
      backend: "memory (demo)",
      user_id: "demo-user",
      is_admin: true,
    }) as unknown as T;
  }

  // ─── Folders ─────────────────────────────────────────────────────────
  if (p === "/api/folders/root/children" && method === "GET") {
    return jsonResp(listChildren(null)) as unknown as T;
  }
  const folderMatch = p.match(/^\/api\/folders\/([^/]+)$/);
  if (folderMatch && method === "GET") {
    const fid = decodeURIComponent(folderMatch[1]);
    const folder = folders.find((f) => f.id === fid);
    if (!folder) throw makeError(404, "folder not found");
    return jsonResp<FolderDetail>({ folder, children: listChildren(fid) }) as unknown as T;
  }
  if (p === "/api/folders" && method === "POST") {
    const body = init.json as { name: string; parent_id: string | null };
    const f: FolderDto = {
      id: id("fld"),
      parent_id: body.parent_id ?? null,
      name: body.name,
      created_at: nowIso(),
      modified_at: nowIso(),
    };
    folders.push(f);
    return jsonResp(f) as unknown as T;
  }

  // ─── Files ───────────────────────────────────────────────────────────
  if (p === "/api/files" && method === "POST") {
    // FormData uploads. Pull `file` and optional `parent_id`.
    const fd = init.body as FormData;
    const file = fd.get("file") as File;
    const parentId = (fd.get("parent_id") as string | null) ?? null;
    const f: DemoFile = {
      id: id("f"),
      parent_id: parentId,
      name: file.name,
      size: file.size,
      content_type: file.type || null,
      version: 1,
      created_at: nowIso(),
      modified_at: nowIso(),
      blob: file,
    };
    files.push(f);
    const { blob, ...dto } = f;
    void blob;
    return jsonResp(dto) as unknown as T;
  }
  const fileMatch = p.match(/^\/api\/files\/([^/]+)(\/(trash|download))?$/);
  if (fileMatch) {
    const fid = decodeURIComponent(fileMatch[1]);
    const sub = fileMatch[3];
    const fIdx = files.findIndex((f) => f.id === fid);
    if (fIdx === -1) throw makeError(404, "file not found");
    if (method === "PATCH" && !sub) {
      const body = init.json as { name: string };
      files[fIdx] = { ...files[fIdx], name: body.name, modified_at: nowIso(), version: files[fIdx].version + 1 };
      const { blob, ...dto } = files[fIdx];
      void blob;
      return jsonResp(dto) as unknown as T;
    }
    if (method === "POST" && sub === "trash") {
      files.splice(fIdx, 1);
      return undefined as T;
    }
  }

  throw makeError(501, `demo: route not implemented (${method} ${p})`);
}

export function demoDownloadUrl(fileId: string): string {
  const f = files.find((x) => x.id === fileId);
  if (f?.blob) return URL.createObjectURL(f.blob);
  // Seeded files have no blob — synthesize a trivial placeholder so the
  // browser actually downloads something the user can open.
  const placeholder = new Blob(
    [`Casual Drive demo · ${f?.name ?? fileId}\n\nThis is placeholder content. The live build serves real bytes.\n`],
    { type: "text/plain" },
  );
  return URL.createObjectURL(placeholder);
}

function makeError(status: number, message: string) {
  const err = new Error(message) as Error & { status: number; body: unknown };
  err.status = status;
  err.body = { error: message };
  return err;
}
