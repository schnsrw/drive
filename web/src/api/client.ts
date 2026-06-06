// Thin fetch wrapper. Same-origin (the SPA and the API live on the app
// origin). Every state-changing call sends X-CSRF-Token when we have one.
//
// In demo mode (VITE_DEMO_MODE=1, GitHub Pages build) requests are routed
// to an in-memory shim instead — see ./demo.ts. Same return types.

import { demoDownloadUrl, demoRequest } from "./demo.ts";

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

export interface Me {
  admin: string;
  backend: string;
  user_id?: string;
  is_admin?: boolean;
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

export async function uploadFile(file: File, parentId: string | null): Promise<FileDto> {
  const fd = new FormData();
  if (parentId) fd.append("parent_id", parentId);
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

export async function trashFile(id: string): Promise<void> {
  return request<void>(`/api/files/${encodeURIComponent(id)}/trash`, {
    method: "POST",
  });
}

export function downloadUrl(id: string): string {
  if (DEMO_MODE) return demoDownloadUrl(id);
  return `/api/files/${encodeURIComponent(id)}/download`;
}
