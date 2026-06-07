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
}

export async function getAbout(): Promise<About> {
  return request<About>("/api/about");
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
}

export interface ListResp {
  folders: FolderDto[];
  files: FileDto[];
}

export interface FolderDetail {
  folder: FolderDto;
  children: ListResp;
}

export async function listRoot(): Promise<ListResp> {
  return request<ListResp>("/api/folders/root/children");
}

export async function getFolder(id: string): Promise<FolderDetail> {
  return request<FolderDetail>(`/api/folders/${encodeURIComponent(id)}`);
}

export async function createFolder(name: string, parentId: string | null): Promise<FolderDto> {
  return request<FolderDto>("/api/folders", {
    method: "POST",
    json: { name, parent_id: parentId },
  });
}

export async function uploadFile(
  file: File,
  parentId: string | null,
  thumbnail?: string | null,
): Promise<FileDto> {
  const fd = new FormData();
  if (parentId) fd.append("parent_id", parentId);
  if (thumbnail) fd.append("thumbnail", thumbnail);
  fd.append("file", file, file.name);
  return request<FileDto>("/api/files", {
    method: "POST",
    body: fd,
  });
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

// ─── Global search ────────────────────────────────────────────────────

export async function searchAll(
  query: string,
  signal?: AbortSignal,
): Promise<ListResp> {
  const params = new URLSearchParams({ q: query, limit: "50" });
  return request<ListResp>(`/api/search?${params.toString()}`, { signal });
}
