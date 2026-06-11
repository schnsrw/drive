// Thin fetch wrapper. Same-origin (the SPA and the API live on the app
// origin). Every state-changing call sends X-CSRF-Token when we have one.
//
// In demo mode (VITE_DEMO_MODE=1, GitHub Pages build) requests are routed
// to an in-memory shim instead — see ./demo.ts. Same return types.

import { demoDownloadUrl, demoRequest, demoShareDownload } from "./demo.ts";

export const DEMO_MODE = import.meta.env.VITE_DEMO_MODE === "1";

export class ApiError extends Error {
  status: number;
  body: unknown;
  constructor(status: number, body: unknown, message?: string) {
    super(message ?? `HTTP ${status}`);
    this.status = status;
    this.body = body;
  }
}

let csrfToken: string | null = null;
export function setCsrfToken(token: string | null) {
  csrfToken = token;
}
export function getCsrfToken() {
  return csrfToken;
}

async function request<T>(
  path: string,
  init: RequestInit & { json?: unknown } = {},
): Promise<T> {
  if (DEMO_MODE) {
    try {
      return await demoRequest<T>(path, init);
    } catch (err) {
      const e = err as Error & { status?: number; body?: unknown };
      throw new ApiError(e.status ?? 500, e.body ?? null, e.message);
    }
  }

  const headers = new Headers(init.headers ?? {});
  let body: BodyInit | null = (init.body as BodyInit | null | undefined) ?? null;

  if (init.json !== undefined) {
    headers.set("content-type", "application/json");
    body = JSON.stringify(init.json);
  }

  const method = (init.method ?? "GET").toUpperCase();
  if (method !== "GET" && method !== "HEAD" && csrfToken) {
    headers.set("x-csrf-token", csrfToken);
  }

  const res = await fetch(path, {
    ...init,
    method,
    headers,
    body,
    credentials: "same-origin",
  });

  if (!res.ok) {
    let parsed: unknown = null;
    try {
      parsed = await res.json();
    } catch {
      try {
        parsed = await res.text();
      } catch {
        parsed = null;
      }
    }
    throw new ApiError(res.status, parsed);
  }
  if (res.status === 204) return undefined as T;
  const ct = res.headers.get("content-type") ?? "";
  if (ct.includes("application/json")) return (await res.json()) as T;
  return (await res.text()) as unknown as T;
}

// ─── Auth ────────────────────────────────────────────────────────────

export interface SignInResp {
  csrf_token: string;
}

export async function signIn(username: string, password: string): Promise<SignInResp> {
  const r = await request<SignInResp>("/api/auth/sign-in", {
    method: "POST",
    json: { username, password },
  });
  setCsrfToken(r.csrf_token);
  return r;
}

export async function signOut(): Promise<void> {
  await request<void>("/api/auth/sign-out", { method: "POST" });
  setCsrfToken(null);
}

export async function changePassword(oldPassword: string, newPassword: string): Promise<void> {
  await request<void>("/api/auth/change-password", {
    method: "POST",
    json: { old_password: oldPassword, new_password: newPassword },
  });
}

export interface About {
  version: string;
  git_sha: string;
  built_at: string;
  license: string;
  repository: string;
  storage_backend: string;
  db_backend: string;
  /** Signed-download URL TTL in seconds — surfaced on Settings → Storage. */
  signed_url_ttl_secs: number;
  /** Per-request body cap in MB — informational. */
  body_limit_mb: number;
}

export async function getAbout(): Promise<About> {
  return request<About>("/api/about");
}

// ─── OIDC sign-in (Phase 3 §12) ──────────────────────────────────────

export interface OidcMetadata {
  enabled: boolean;
  /** Human label for the IdP button. Present when `enabled`. */
  provider_label?: string;
  /** When false, the password sign-in form is hidden — the server also
   * returns 404 on /api/auth/sign-in to make sure clients can't bypass. */
  allow_password_auth: boolean;
}

export async function oidcMetadata(): Promise<OidcMetadata> {
  return request<OidcMetadata>("/api/auth/oidc/metadata");
}

/** Browser-side helper — server is the source of truth for the URL,
 * so this just navigates to the login route which 302s onward. */
export function oidcLoginUrl(): string {
  return "/api/auth/oidc/login";
}

// ─── First-run setup ─────────────────────────────────────────────────

export interface SetupStatus {
  needs_setup: boolean;
}

export async function setupStatus(): Promise<SetupStatus> {
  return request<SetupStatus>("/api/setup/status");
}

export async function setupAdmin(username: string, password: string): Promise<SignInResp> {
  const r = await request<SignInResp>("/api/setup/admin", {
    method: "POST",
    json: { username, password },
  });
  setCsrfToken(r.csrf_token);
  return r;
}

// ─── Sharing ─────────────────────────────────────────────────────────

export interface ShareDto {
  id: string;
  token: string;
  url: string;
  permissions: string;
  has_password: boolean;
  expires_at: string | null;
  created_at: string;
  last_accessed_at: string | null;
  access_count: number;
}

export interface CreateShareBody {
  permissions?: "view";
  password?: string | null;
  expires_in_seconds?: number | null;
}

export async function createShare(fileId: string, body: CreateShareBody): Promise<ShareDto> {
  return request<ShareDto>(`/api/files/${encodeURIComponent(fileId)}/share`, {
    method: "POST",
    json: body,
  });
}

export async function listShares(fileId: string): Promise<{ shares: ShareDto[] }> {
  return request<{ shares: ShareDto[] }>(`/api/files/${encodeURIComponent(fileId)}/shares`);
}

export async function revokeShare(shareId: string): Promise<void> {
  await request<void>(`/api/shares/${encodeURIComponent(shareId)}`, { method: "DELETE" });
}

export interface ResolvedShare {
  file: {
    name: string;
    size: number;
    content_type: string | null;
    modified_at: string;
  };
  download_url: string;
  permissions: string;
}

export async function resolveShare(token: string, password?: string | null): Promise<ResolvedShare> {
  return request<ResolvedShare>(`/api/share/${encodeURIComponent(token)}`, {
    method: "POST",
    json: { password: password ?? null },
  });
}

// ─── Activity feed ──────────────────────────────────────────────────

export interface ActivityEvent {
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

export interface ActivityPage {
  events: ActivityEvent[];
  next_before: string | null;
}

export async function getActivity(before?: string | null, limit = 50): Promise<ActivityPage> {
  const params = new URLSearchParams();
  if (before) params.set("before", before);
  params.set("limit", String(limit));
  return request<ActivityPage>(`/api/activity?${params.toString()}`);
}

export function shareDownloadUrl(token: string): string {
  if (DEMO_MODE) {
    return demoShareDownload(token) ?? `/api/share/${encodeURIComponent(token)}/download`;
  }
  return `/api/share/${encodeURIComponent(token)}/download`;
}

export interface Me {
  admin: string;
  backend: string;
  user_id?: string;
  is_admin?: boolean;
  used_bytes?: number;
  quota_bytes?: number | null;
}

export async function me(): Promise<Me> {
  return request<Me>("/api/me");
}

// ─── Files + Folders ────────────────────────────────────────────────

export interface FolderDto {
  id: string;
  parent_id: string | null;
  name: string;
  created_at: string;
  modified_at: string;
}

export interface FileDto {
  id: string;
  parent_id: string | null;
  name: string;
  size: number;
  content_type: string | null;
  version: number;
  created_at: string;
  modified_at: string;
  /** Client-generated data URI for image uploads (pipeline §5.2). */
  thumbnail?: string | null;
  /** Lifecycle (pipeline §13.6). Direct uploads sit at `uploading` until
   * the SPA's `complete` call flips them to `ready`. The Files surface
   * filters out non-ready rows by default. */
  status?: "uploading" | "ready" | "failed";
  /** Server-side thumbnail generation state (pipeline §5.4). The SPA
   * uses `thumb_urls` (below) when this is `ready`. */
  thumbs_state?: "pending" | "ready" | "unsupported" | "failed";
  /** Convenience URLs for the three thumbnail sizes. Populated when
   * `thumbs_state === "ready"`. */
  thumb_urls?: { small: string; medium: string; large: string };
}

export interface ListResp {
  folders: FolderDto[];
  files: FileDto[];
}

export interface FolderDetail {
  folder: FolderDto;
  children: ListResp;
}

export async function listRoot(workspaceId?: string | null): Promise<ListResp> {
  const qs = workspaceId ? `?workspace=${encodeURIComponent(workspaceId)}` : "";
  return request<ListResp>(`/api/folders/root/children${qs}`);
}

export async function getFolder(id: string): Promise<FolderDetail> {
  return request<FolderDetail>(`/api/folders/${encodeURIComponent(id)}`);
}

export async function createFolder(
  name: string,
  parentId: string | null,
  workspaceId?: string | null,
): Promise<FolderDto> {
  return request<FolderDto>("/api/folders", {
    method: "POST",
    json: { name, parent_id: parentId, workspace_id: workspaceId ?? undefined },
  });
}

/// 8 MiB — files at or above this threshold try the direct-to-storage
/// path first (pipeline §13.6). Smaller files go via the proxy multipart
/// upload — the round-trip is cheaper than the extra metadata hop.
const DIRECT_UPLOAD_THRESHOLD = 8 * 1024 * 1024;

const DIRECT_UPLOAD_ENABLED =
  (typeof import.meta !== "undefined" &&
    Boolean((import.meta as { env?: Record<string, unknown> }).env?.VITE_DIRECT_UPLOAD)) ||
  false;

export async function uploadFile(
  file: File,
  parentId: string | null,
  thumbnail?: string | null,
  workspaceId?: string | null,
): Promise<FileDto> {
  // Direct path for large files when the workspace's storage supports it.
  // Server returns 409 on adapters that can't presign (fs / memory) —
  // we catch that and fall through to the proxy.
  if (DIRECT_UPLOAD_ENABLED && file.size >= DIRECT_UPLOAD_THRESHOLD) {
    try {
      return await uploadDirect(file, parentId, thumbnail, workspaceId);
    } catch (e) {
      const err = e as ApiError;
      // 409 = adapter can't presign; 0 / network failure = CORS or
      // similar. Either way, the proxy path always works.
      if (err.status === undefined || err.status === 409 || err.status === 0) {
        console.warn("direct upload fell back to proxy:", err.message ?? err);
      } else {
        throw e;
      }
    }
  }
  return uploadViaProxy(file, parentId, thumbnail, workspaceId);
}

async function uploadViaProxy(
  file: File,
  parentId: string | null,
  thumbnail?: string | null,
  workspaceId?: string | null,
): Promise<FileDto> {
  const fd = new FormData();
  if (parentId) fd.append("parent_id", parentId);
  if (workspaceId) fd.append("workspace_id", workspaceId);
  if (thumbnail) fd.append("thumbnail", thumbnail);
  fd.append("file", file, file.name);
  return request<FileDto>("/api/files", {
    method: "POST",
    body: fd,
  });
}

interface PresignResp {
  file_id: string;
  upload_url: string;
  expires_at: string;
  method: "PUT";
  required_headers: Record<string, string>;
}

async function uploadDirect(
  file: File,
  parentId: string | null,
  thumbnail: string | null | undefined,
  workspaceId: string | null | undefined,
): Promise<FileDto> {
  const pre = await request<PresignResp>("/api/files/upload-url", {
    method: "POST",
    json: {
      name: file.name,
      size: file.size,
      content_type: file.type || undefined,
      parent_id: parentId ?? undefined,
      workspace_id: workspaceId ?? undefined,
    },
  });

  // PUT bytes directly to the bucket. If this fails, abort the row so
  // we don't leak an `uploading` placeholder forever.
  try {
    const putResp = await fetch(pre.upload_url, {
      method: pre.method,
      headers: pre.required_headers,
      body: file,
      mode: "cors",
    });
    if (!putResp.ok) {
      throw new Error(`upload PUT failed: ${putResp.status}`);
    }
  } catch (e) {
    await request<void>(`/api/files/${encodeURIComponent(pre.file_id)}/abort`, {
      method: "POST",
    }).catch(() => undefined);
    throw e;
  }

  // Finalize. Thumbnail (if provided by the client) gets posted to a
  // future endpoint; for now we accept the limitation that direct
  // uploads ship without a client-side thumb. The server-side §5.4
  // worker fills in real ones lazily.
  void thumbnail;
  return request<FileDto>(`/api/files/${encodeURIComponent(pre.file_id)}/complete`, {
    method: "POST",
    json: {},
  });
}

/** `GET /api/files/{id}` — fetch a single file's metadata. Used by
 *  the fullscreen `/file/<id>` route when it's loaded cold (refresh /
 *  shared URL / bookmark) without a FileDto in `history.state`. */
export async function getFile(id: string): Promise<FileDto> {
  return request<FileDto>(`/api/files/${encodeURIComponent(id)}`);
}

export async function renameFile(id: string, name: string): Promise<FileDto> {
  return request<FileDto>(`/api/files/${encodeURIComponent(id)}`, {
    method: "PATCH",
    json: { name },
  });
}

export async function renameFolder(id: string, name: string): Promise<FolderDto> {
  return request<FolderDto>(`/api/folders/${encodeURIComponent(id)}`, {
    method: "PATCH",
    json: { name },
  });
}

export async function trashFile(id: string): Promise<void> {
  return request<void>(`/api/files/${encodeURIComponent(id)}/trash`, {
    method: "POST",
  });
}

export function downloadUrl(id: string): string {
  if (DEMO_MODE) return demoDownloadUrl(id);
  return `/api/files/${encodeURIComponent(id)}/download`;
}

// ─── Editor handoff (WOPI) ────────────────────────────────────────────

export interface OpenResp {
  editor_app: "sheet" | "document";
  entry_url: string;
  access_token: string;
  access_token_ttl: number;
  wopi_src: string;
}

export async function openInEditor(fileId: string): Promise<OpenResp> {
  return request<OpenResp>(`/api/files/${encodeURIComponent(fileId)}/open`);
}

// ─── Admin ─────────────────────────────────────────────────────────────

export interface AdminSystem {
  version: string;
  git_sha: string;
  built_at: string;
  license: string;
  storage_backend: string;
  storage_config: {
    fs_root: string | null;
    s3_bucket: string | null;
    s3_endpoint: string | null;
    s3_region: string | null;
  };
  db_backend: string;
  uptime_seconds: number;
  active_sessions: number;
  healthy: boolean;
  recent_sign_ins: { actor_username: string | null; ok: boolean; at: string }[];
}

export async function getAdminSystem(): Promise<AdminSystem> {
  return request<AdminSystem>("/api/admin/system");
}

// ─── Admin: user management + quota allocation ────────────────────────

export interface AdminUser {
  id: string;
  username: string;
  is_admin: boolean;
  created_at: string;
  used_bytes: number;
  quota_bytes: number | null;
}

export async function listAdminUsers(): Promise<{ users: AdminUser[] }> {
  return request<{ users: AdminUser[] }>("/api/admin/users");
}

export async function createAdminUser(input: {
  username: string;
  password: string;
  is_admin?: boolean;
  quota_bytes?: number | null;
}): Promise<AdminUser> {
  return request<AdminUser>("/api/admin/users", {
    method: "POST",
    json: input,
  });
}

export async function setUserQuota(
  userId: string,
  quotaBytes: number | null,
): Promise<void> {
  await request<void>(`/api/admin/users/${encodeURIComponent(userId)}/quota`, {
    method: "PATCH",
    json: { quota_bytes: quotaBytes },
  });
}

export async function requestQuotaUpgrade(
  requestedBytes?: number | null,
  reason?: string | null,
): Promise<void> {
  await request<void>("/api/me/quota/request", {
    method: "POST",
    json: {
      requested_bytes: requestedBytes ?? null,
      reason: reason ?? null,
    },
  });
}

// ─── Workspaces ───────────────────────────────────────────────────────

export interface Workspace {
  id: string;
  name: string;
  kind: "personal" | "team";
  owner_id: string;
  role: "owner" | "member";
  member_count: number;
  created_at: string;
}

export interface WorkspaceListResp {
  current_id: string;
  workspaces: Workspace[];
}

export async function listWorkspaces(): Promise<WorkspaceListResp> {
  return request<WorkspaceListResp>("/api/workspaces");
}

export interface WorkspaceMember {
  user_id: string;
  username: string;
  is_admin: boolean;
  role: "owner" | "member";
  joined_at: string;
}

export interface MembersResp {
  members: WorkspaceMember[];
}

export async function listWorkspaceMembers(workspaceId: string): Promise<MembersResp> {
  return request<MembersResp>(`/api/workspaces/${encodeURIComponent(workspaceId)}/members`);
}

export async function createWorkspace(name: string): Promise<Workspace> {
  return request<Workspace>("/api/workspaces", {
    method: "POST",
    json: { name },
  });
}

export async function renameWorkspace(id: string, name: string): Promise<void> {
  await request<void>(`/api/workspaces/${encodeURIComponent(id)}`, {
    method: "PATCH",
    json: { name },
  });
}

export async function deleteWorkspace(id: string): Promise<void> {
  await request<void>(`/api/workspaces/${encodeURIComponent(id)}`, {
    method: "DELETE",
  });
}

export async function transferWorkspace(id: string, newOwnerId: string): Promise<void> {
  await request<void>(`/api/workspaces/${encodeURIComponent(id)}/transfer`, {
    method: "POST",
    json: { new_owner_id: newOwnerId },
  });
}

// ─── Workspace storage (BYO) — pipeline §8.9 ──────────────────────────

export type ByoProvider = "s3" | "minio" | "r2" | "b2";

export interface ByoStatusDefault {
  kind: "default";
}

export interface ByoStatusActive {
  kind: "byo";
  id: string;
  provider: ByoProvider;
  bucket: string;
  region: string;
  endpoint: string | null;
  access_key_id_masked: string;
  secret_masked: string;
  key_version: number;
  tested_at: string | null;
  tested_ok: boolean;
  tested_error: string | null;
}

export type ByoStatus = ByoStatusDefault | ByoStatusActive;

export interface ByoConfigInput {
  provider: ByoProvider;
  bucket: string;
  region: string;
  endpoint?: string;
  access_key_id: string;
  secret_access_key: string;
}

export interface ByoTestResult {
  ok: boolean;
  latency_ms?: number;
  error?: string;
}

export async function getWorkspaceStorage(id: string): Promise<ByoStatus> {
  return request<ByoStatus>(
    `/api/workspaces/${encodeURIComponent(id)}/storage`,
  );
}

export async function testWorkspaceStorage(
  id: string,
  cfg: ByoConfigInput,
): Promise<ByoTestResult> {
  return request<ByoTestResult>(
    `/api/workspaces/${encodeURIComponent(id)}/storage/test`,
    { method: "POST", json: cfg },
  );
}

export async function saveWorkspaceStorage(
  id: string,
  cfg: ByoConfigInput,
): Promise<ByoStatusActive> {
  return request<ByoStatusActive>(
    `/api/workspaces/${encodeURIComponent(id)}/storage`,
    { method: "PUT", json: cfg },
  );
}

export async function replaceWorkspaceStorageCredentials(
  id: string,
  accessKeyId: string,
  secretAccessKey: string,
): Promise<void> {
  await request<void>(
    `/api/workspaces/${encodeURIComponent(id)}/storage/credentials`,
    {
      method: "PATCH",
      json: { access_key_id: accessKeyId, secret_access_key: secretAccessKey },
    },
  );
}

export async function removeWorkspaceStorage(id: string): Promise<void> {
  await request<void>(`/api/workspaces/${encodeURIComponent(id)}/storage`, {
    method: "DELETE",
  });
}

// ─── Notes / Wiki — pipeline §8.11 ─────────────────────────────────────

export interface NoteNode {
  id: string;
  parent_id: string | null;
  title: string;
  order_key: string;
}

export interface NoteBacklink {
  id: string;
  title: string;
}

export interface Note {
  id: string;
  workspace_id: string;
  parent_id: string | null;
  title: string;
  body: string;
  order_key: string;
  created_at: string;
  modified_at: string;
  backlinks: NoteBacklink[];
}

export interface NotesTreeResp {
  workspace_id: string;
  nodes: NoteNode[];
  trashed: NoteNode[];
}

export async function notesTree(workspaceId?: string | null): Promise<NotesTreeResp> {
  const qs = workspaceId ? `?workspace=${encodeURIComponent(workspaceId)}` : "";
  return request<NotesTreeResp>(`/api/notes/tree${qs}`);
}

export async function noteGet(id: string): Promise<Note> {
  return request<Note>(`/api/notes/${encodeURIComponent(id)}`);
}

export async function noteCreate(
  title: string,
  parentId?: string | null,
  workspaceId?: string | null,
): Promise<Note> {
  return request<Note>("/api/notes", {
    method: "POST",
    json: {
      title,
      parent_id: parentId ?? undefined,
      workspace_id: workspaceId ?? undefined,
    },
  });
}

export async function notePatch(
  id: string,
  patch: {
    title?: string;
    body?: string;
    /** `null` moves to root; omit to leave unchanged. */
    parent_id?: string | null;
    order_key?: string;
  },
): Promise<Note> {
  return request<Note>(`/api/notes/${encodeURIComponent(id)}`, {
    method: "PATCH",
    json: patch,
  });
}

export async function noteTrash(id: string): Promise<void> {
  await request<void>(`/api/notes/${encodeURIComponent(id)}/trash`, { method: "POST" });
}

export async function noteRestore(id: string): Promise<void> {
  await request<void>(`/api/notes/${encodeURIComponent(id)}/restore`, { method: "POST" });
}

export async function noteDelete(id: string): Promise<void> {
  await request<void>(`/api/notes/${encodeURIComponent(id)}`, { method: "DELETE" });
}

export async function notesSearch(
  q: string,
  workspaceId?: string | null,
  signal?: AbortSignal,
): Promise<NoteNode[]> {
  const params = new URLSearchParams({ q });
  if (workspaceId) params.set("workspace", workspaceId);
  return request<NoteNode[]>(`/api/notes/search?${params.toString()}`, { signal });
}

// ─── Global search ────────────────────────────────────────────────────

/** Lightweight shape used by the Cmd-K palette + back-compat callers. */
export async function searchAll(
  query: string,
  signal?: AbortSignal,
  workspaceId?: string | null,
): Promise<ListResp> {
  const params = new URLSearchParams({ q: query, limit: "50" });
  if (workspaceId) params.set("workspace", workspaceId);
  return request<ListResp>(`/api/search?${params.toString()}`, { signal });
}

// ─── Global search — Phase 3 wire shape ───────────────────────────────
// Spec: docs/ux/12-search-surface.md + docs/research/16-scale-infra.md
// §"Search backend wire contract".

export type SortBy = "relevance" | "modified" | "created" | "name" | "size";
export type SortDir = "asc" | "desc";
export type SearchScope = "folder" | "workspace" | "all";

/** Canonical content-type buckets the chip row offers. */
export type TypeBucket =
  | "folder"
  | "document"
  | "spreadsheet"
  | "pdf"
  | "image"
  | "video"
  | "audio"
  | "markdown"
  | "archive"
  | "other"
  | "note";

export interface SearchFilters {
  /** Empty string allowed when at least one other filter is set. */
  q: string;
  scope: SearchScope;
  /** Only meaningful when scope == "folder". */
  folder_id?: string;
  /** When omitted, server defaults to the caller's active workspace. */
  workspace_ids?: string[];
  types: TypeBucket[];
  owner_ids: string[];
  modified_after?: string;  // RFC3339
  modified_before?: string;
  created_after?: string;
  created_before?: string;
  size_min?: number;
  size_max?: number;
  has_share_link?: boolean;
  /** `true` ⇒ include trashed alongside non-trashed; default excludes. */
  include_trashed?: boolean;
}

export interface SearchPaging {
  sort: SortBy;
  sort_dir: SortDir;
  limit?: number;
  /** Opaque cursor from the previous response's `next_cursor`. */
  after?: string;
}

export interface NoteSearchHit {
  id: string;
  parent_id: string | null;
  title: string;
}

export interface SearchTotals {
  files: number;
  folders: number;
  notes: number;
  exact: boolean;
}

export interface SearchResp {
  files: FileDto[];
  folders: FolderDto[];
  notes: NoteSearchHit[];
  total: SearchTotals;
  next_cursor?: string | null;
  /** What the backend actually sorted by — relevance falls back to
   * "modified" on the sqlite path. The SPA greys out the Relevance
   * option in the Sort popover when this doesn't match the requested. */
  sort_applied: SortBy;
}

/** Phase 3 search call. Empty filters + non-empty `q` is the floor;
 * filters + sort + pagination are layered on. */
export async function searchAdvanced(
  filters: SearchFilters,
  paging: SearchPaging,
  signal?: AbortSignal,
): Promise<SearchResp> {
  const p = new URLSearchParams();
  if (filters.q) p.set("q", filters.q);
  if (filters.scope) p.set("scope", filters.scope);
  if (filters.folder_id) p.set("folder_id", filters.folder_id);
  if (filters.workspace_ids?.length) p.set("workspace", filters.workspace_ids.join(","));
  if (filters.types.length) p.set("type", filters.types.join(","));
  if (filters.owner_ids.length) p.set("owner", filters.owner_ids.join(","));
  if (filters.modified_after) p.set("modified_after", filters.modified_after);
  if (filters.modified_before) p.set("modified_before", filters.modified_before);
  if (filters.created_after) p.set("created_after", filters.created_after);
  if (filters.created_before) p.set("created_before", filters.created_before);
  if (filters.size_min !== undefined) p.set("size_min", String(filters.size_min));
  if (filters.size_max !== undefined) p.set("size_max", String(filters.size_max));
  if (filters.has_share_link !== undefined) p.set("has_share_link", String(filters.has_share_link));
  if (filters.include_trashed) p.set("include_trashed", "true");
  p.set("sort", paging.sort);
  p.set("sort_dir", paging.sort_dir);
  p.set("limit", String(paging.limit ?? 30));
  if (paging.after) p.set("after", paging.after);
  return request<SearchResp>(`/api/search?${p.toString()}`, { signal });
}

/** Build a default filter object — the SPA's "no filters set" state. */
export function defaultFilters(scope: SearchScope = "workspace"): SearchFilters {
  return {
    q: "",
    scope,
    types: [],
    owner_ids: [],
  };
}

/** True when any user-visible filter is active (i.e. would render a
 * chip in its active state). Excludes the query itself. */
export function hasActiveFilters(f: SearchFilters): boolean {
  return (
    f.types.length > 0 ||
    f.owner_ids.length > 0 ||
    !!f.modified_after || !!f.modified_before ||
    !!f.created_after || !!f.created_before ||
    f.size_min !== undefined || f.size_max !== undefined ||
    f.has_share_link !== undefined ||
    !!f.include_trashed ||
    (f.workspace_ids?.length ?? 0) > 0
  );
}
