/**
 * RT2 — SPA presence client.
 *
 * Spec: docs/research/14-presence.md §"SPA surface".
 *
 * One provider mounted at the shell root. While the user has a session
 * AND a workspace selected, it:
 *
 *   - subscribes to `GET /api/presence/{workspaceId}` via EventSource
 *     (auto-reconnects on disconnect; the only thing we own there is
 *     re-applying the heartbeat schedule);
 *   - POSTs `/beat` every 25 s with the file currently being
 *     "viewed" (preview modal / editor handoff in progress);
 *   - POSTs `/leave` on `pagehide` via `sendBeacon` so the avatar
 *     drops immediately on tab close;
 *   - reduces `present` / `left` / `action` events into a
 *     `Map<user_id, PresenceUser>` that hooks consume.
 *
 * Self-exclusion: hooks never surface the current user — the SPA's
 * job is "who ELSE is here", not "what's my own avatar."
 *
 * Multi-tab handling: the SPA opens one EventSource per tab; the
 * server collapses entries by user_id so multiple tabs from the same
 * user are one entry. The TTL sweep takes care of stale tabs.
 */
import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";

import { DEMO_MODE } from "../api/client.ts";
import { useAuth } from "../auth/AuthContext.tsx";
import { useActiveWorkspaceId } from "./WorkspaceContext.tsx";

/** Wire shape of a presence row in the SPA. Matches the server's
 * `PresenceEvent::Present` payload one-for-one. */
export interface PresenceUser {
  user_id: string;
  username: string;
  tint: string;
  /** File id the user is currently viewing (preview modal open or
   * editor handoff in progress). `null` means "active but not pinned
   * to any file". */
  viewing: string | null;
  /** Client-side wall-clock the row was last touched. Drives the
   * "active 14 s ago" tooltip + sort-most-recent-first ordering. */
  last_seen: number;
}

/** Action event broadcast from the server when a peer modifies a
 * file or folder in this workspace. Consumers (RT4's quiet toast,
 * sidebar refresh) subscribe via `usePresenceActions`. */
export interface PresenceAction {
  user_id: string;
  action: string;
  target_id: string | null;
  target_name: string | null;
  /** Client-side wall-clock the action was received. */
  received_at: number;
}

interface PresenceCtx {
  /** All currently-present users EXCEPT the caller themselves. */
  users: PresenceUser[];
  /** Tell the server which file (if any) we're currently viewing.
   * Re-beats immediately so peers see the change without waiting
   * for the 25-s heartbeat tick. `null` clears the viewing-pin. */
  setViewing: (fileId: string | null) => void;
  /** Read-only running tally of action events. RT4's toast hooks
   * into this; passive consumers can ignore. */
  actions: PresenceAction[];
}

const Ctx = createContext<PresenceCtx | null>(null);

/** Heartbeat cadence — server TTL is 60 s, one missed beat is fine. */
const BEAT_INTERVAL_MS = 25_000;

/** Cap on the buffered actions list. Old entries get dropped — the
 * SPA's only consumer (RT4 toast) reacts to the newest one and
 * forgets. */
const ACTIONS_BUFFER = 50;

export function PresenceProvider({ children }: { children: ReactNode }) {
  const { status } = useAuth();
  const workspaceId = useActiveWorkspaceId();
  const myId = status.kind === "authed" ? status.me.user_id ?? null : null;

  const [users, setUsers] = useState<Map<string, PresenceUser>>(() => new Map());
  const [actions, setActions] = useState<PresenceAction[]>([]);
  // Stashed inside a ref so the heartbeat interval (frozen in a
  // closure on first effect run) can read the LATEST viewing value
  // without forcing the interval to tear down + restart on every
  // change.
  const viewingRef = useRef<string | null>(null);

  // Stable `setViewing` so callers can pass it through useEffect deps
  // without causing churn. Skipped entirely in DEMO_MODE — the demo
  // backend has no presence endpoints and Vite's proxy would spam
  // 500s into the console (which the `_iframe-verify` e2e treats as
  // test failures).
  const beatNow = useCallback(
    (ws: string, viewing: string | null) => {
      if (DEMO_MODE) return;
      // Fire-and-forget. The server doesn't 4xx anything but a
      // membership 403 — and we'd only see that on a workspace
      // mid-revoke, which the sidebar refresh handles separately.
      void fetch(`/api/presence/${encodeURIComponent(ws)}/beat`, {
        method: "POST",
        credentials: "include",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ viewing }),
      }).catch(() => {
        /* offline — next interval tick will retry */
      });
    },
    [],
  );

  const setViewing = useCallback(
    (fileId: string | null) => {
      viewingRef.current = fileId;
      if (workspaceId) beatNow(workspaceId, fileId);
    },
    [workspaceId, beatNow],
  );

  // EventSource subscription + heartbeat lifecycle. Re-runs on
  // workspace change so a switch tears down the old stream and
  // opens the new one — server membership-gates each.
  useEffect(() => {
    if (status.kind !== "authed" || !workspaceId) {
      setUsers(new Map());
      setActions([]);
      viewingRef.current = null;
      return;
    }
    // Demo mode runs against the in-memory client shim — no real
    // presence backend exists. Skip the subscription so we don't
    // spam EventSource reconnect attempts against a 404.
    if (DEMO_MODE) return;

    const wsPath = `/api/presence/${encodeURIComponent(workspaceId)}`;
    const es = new EventSource(wsPath);

    // First beat fires immediately so we appear in OUR OWN stream's
    // initial burst — without it the user only sees themselves after
    // the first 25-s tick lands.
    beatNow(workspaceId, viewingRef.current);

    const heartbeat = window.setInterval(() => {
      beatNow(workspaceId, viewingRef.current);
    }, BEAT_INTERVAL_MS);

    es.addEventListener("present", (e) => {
      try {
        const data = JSON.parse((e as MessageEvent).data) as {
          user_id: string;
          username: string;
          tint: string;
          viewing: string | null;
        };
        setUsers((prev) => {
          const next = new Map(prev);
          next.set(data.user_id, {
            user_id: data.user_id,
            username: data.username,
            tint: data.tint,
            viewing: data.viewing,
            last_seen: Date.now(),
          });
          return next;
        });
      } catch {
        /* drop malformed event */
      }
    });

    es.addEventListener("left", (e) => {
      try {
        const data = JSON.parse((e as MessageEvent).data) as { user_id: string };
        setUsers((prev) => {
          if (!prev.has(data.user_id)) return prev;
          const next = new Map(prev);
          next.delete(data.user_id);
          return next;
        });
      } catch {
        /* drop */
      }
    });

    es.addEventListener("action", (e) => {
      try {
        const data = JSON.parse((e as MessageEvent).data) as {
          user_id: string;
          action: string;
          target_id: string | null;
          target_name: string | null;
        };
        setActions((prev) => {
          const next = [
            {
              user_id: data.user_id,
              action: data.action,
              target_id: data.target_id,
              target_name: data.target_name,
              received_at: Date.now(),
            },
            ...prev,
          ];
          if (next.length > ACTIONS_BUFFER) next.length = ACTIONS_BUFFER;
          return next;
        });
      } catch {
        /* drop */
      }
    });

    // EventSource auto-reconnects on transient errors; we just log
    // for visibility and let the browser handle the backoff.
    es.onerror = () => {
      // Quiet — a network hiccup shouldn't spam the console.
    };

    // Leave-on-unload. `sendBeacon` survives the tab close where a
    // regular fetch would be aborted. Best-effort; if the browser
    // drops it, the 60-s sweep catches up.
    function onPageHide() {
      const url = `/api/presence/${encodeURIComponent(workspaceId!)}/leave`;
      if (navigator.sendBeacon) {
        navigator.sendBeacon(url);
      } else {
        // Older browsers — fire-and-forget; the request may not
        // complete but we tried.
        void fetch(url, {
          method: "POST",
          credentials: "include",
          keepalive: true,
        }).catch(() => {});
      }
    }
    window.addEventListener("pagehide", onPageHide);

    return () => {
      window.clearInterval(heartbeat);
      window.removeEventListener("pagehide", onPageHide);
      es.close();
      // Synchronous-ish leave on workspace-switch / sign-out — same
      // sendBeacon path so the avatar drops immediately for peers.
      const leaveUrl = `/api/presence/${encodeURIComponent(workspaceId)}/leave`;
      if (navigator.sendBeacon) {
        navigator.sendBeacon(leaveUrl);
      }
      setUsers(new Map());
    };
  }, [status.kind, workspaceId, beatNow]);

  const value = useMemo<PresenceCtx>(() => {
    // Drop self before exposing — the avatar stack shows "who else."
    const others: PresenceUser[] = [];
    for (const u of users.values()) {
      if (u.user_id !== myId) others.push(u);
    }
    others.sort((a, b) => b.last_seen - a.last_seen);
    return { users: others, setViewing, actions };
  }, [users, myId, setViewing, actions]);

  return <Ctx.Provider value={value}>{children}</Ctx.Provider>;
}

/** All currently-present users in the active workspace EXCEPT the
 * caller. Sorted most-recently-active first. */
export function usePresenceUsers(): PresenceUser[] {
  return useCtx().users;
}

/** Returns the first peer (excluding self) who's currently viewing
 * the given file id, or `null`. Used by RT3's file-row dot. */
export function useViewingFile(fileId: string | null | undefined): PresenceUser | null {
  const users = useCtx().users;
  if (!fileId) return null;
  return users.find((u) => u.viewing === fileId) ?? null;
}

/** Imperative setter for the caller's own "viewing this file" pin.
 * Wire from the preview modal / editor mount so peers see what
 * you're looking at. Pass `null` to clear. */
export function useReportViewing(): (fileId: string | null) => void {
  return useCtx().setViewing;
}

/** Buffered action events. RT4 toast subscribes; passive consumers
 * can ignore. The newest event is index 0. */
export function usePresenceActions(): PresenceAction[] {
  return useCtx().actions;
}

function useCtx(): PresenceCtx {
  const v = useContext(Ctx);
  if (!v) {
    // Defensive fallback — components that mount before the provider
    // (e.g. error boundaries during boot) see an empty presence list
    // instead of a crash. Real codepaths always have the provider.
    return { users: [], actions: [], setViewing: () => {} };
  }
  return v;
}
