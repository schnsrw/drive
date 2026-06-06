import { useState } from "react";

import { DEMO_MODE } from "../api/client.ts";
import { useAuth } from "../auth/AuthContext.tsx";
import { ComingSoon } from "../components/ComingSoon.tsx";
import { DemoBanner } from "../components/DemoBanner.tsx";
import { EmptyState } from "../components/EmptyState.tsx";
import { Sidebar, type NavId } from "../components/Sidebar.tsx";
import { TopBar, type ViewMode } from "../components/TopBar.tsx";
import { Files } from "./Files.tsx";

export function Shell() {
  const { status } = useAuth();
  const username = status.kind === "authed" ? status.me.admin : "admin";
  const [nav, setNav] = useState<NavId>("home");
  const [view, setView] = useState<ViewMode>("grid");
  const [query, setQuery] = useState("");
  const [itemCount, setItemCount] = useState(0);
  const [uploadTick, setUploadTick] = useState(0);
  const [newFolderTick, setNewFolderTick] = useState(0);

  return (
    <div className="h-full w-full flex flex-col" style={{ background: "var(--paper)" }}>
      {DEMO_MODE && <DemoBanner />}
      <div className="flex" style={{ flex: 1, minHeight: 0 }}>
      <Sidebar
        current={nav}
        onSelect={setNav}
        itemCount={itemCount}
        onNewFolder={() => setNewFolderTick((t) => t + 1)}
        onUpload={() => setUploadTick((t) => t + 1)}
        username={username}
      />
      <div className="flex-1 flex flex-col" style={{ minWidth: 0 }}>
        {nav === "home" && (
          <div style={{ padding: "26px 40px 0" }}>
            <TopBar query={query} onQueryChange={setQuery} view={view} onViewChange={setView} />
          </div>
        )}
        <main style={{ flex: 1, display: "flex", flexDirection: "column", minHeight: 0 }}>
          {nav === "home" && (
            <Files
              view={view}
              query={query}
              uploadRequested={uploadTick}
              onUploadHandled={() => {}}
              newFolderRequested={newFolderTick}
              onNewFolderHandled={() => {}}
              onItemCount={setItemCount}
            />
          )}
          {nav === "recent" && (
            <CenteredPane>
              <ComingSoon
                title="Recently opened files"
                description="See the last 20 files you opened — across every folder — at the top of your Drive."
                bullets={[
                  "Auto-tracks open events and snapshots them per user",
                  "Filterable by type and date",
                  "Persists across sessions",
                ]}
              />
            </CenteredPane>
          )}
          {nav === "starred" && (
            <CenteredPane>
              <ComingSoon
                title="Starred files and folders"
                description="Pin the things you keep coming back to. Stars work across folders and survive renames."
                bullets={[
                  "Star/unstar from the preview modal or context menu",
                  "Star a folder to pin the whole tree",
                  "Synced across sessions and devices once multi-user lands",
                ]}
              />
            </CenteredPane>
          )}
          {nav === "shared" && (
            <CenteredPane>
              <ComingSoon
                title="Shared with you"
                description="Files other members of your workspace share with you appear here — ranked by recent activity."
                bullets={[
                  "View files shared via direct invite or share-link",
                  "Filter by sender and permission level (view / comment / edit)",
                  "Multi-user is queued for v0.2",
                ]}
              />
            </CenteredPane>
          )}
          {nav === "trash" && (
            <CenteredPane>
              <EmptyState
                title="Trash is empty."
                subtitle="Files you delete will appear here for 30 days before being permanently removed."
              />
            </CenteredPane>
          )}
          {nav === "activity" && (
            <CenteredPane>
              <ComingSoon
                title="Activity & audit log"
                description="Tamper-evident event feed for everything that happens in your Drive — sign-ins, uploads, downloads, shares, deletions."
                bullets={[
                  "Grouped by day, type-tagged, owner-filterable",
                  "Append-only audit_log table — required for compliance",
                  "Per-action exportable JSON for downstream SIEMs",
                ]}
              />
            </CenteredPane>
          )}
          {nav === "admin" && (
            <CenteredPane>
              <ComingSoon
                title="Admin dashboard"
                description="System health, storage backend status, active sessions, cache/indexing — at a glance for the instance operator."
                bullets={[
                  "Storage adapter status (fs / S3 / MinIO) + quota",
                  "Active sessions, recent failed sign-ins",
                  "OpenSearch + Redis dashboards when enabled",
                  "One-click ClamAV toggle once the scanner ships",
                ]}
              />
            </CenteredPane>
          )}
          {nav === "settings" && (
            <CenteredPane>
              <ComingSoon
                title="Settings"
                description="Sectioned configuration — Account, Workspace, Members, Roles, Sharing, Storage, Notifications, API tokens, Audit log, About."
                bullets={[
                  "Change admin password (real for v0)",
                  "Storage backend + quota readout (real for v0)",
                  "Members + Roles + Invitations (v0.2 — multi-user)",
                  "API token management (v0.2)",
                ]}
              />
            </CenteredPane>
          )}
        </main>
      </div>
      </div>
    </div>
  );
}

function CenteredPane({ children }: { children: React.ReactNode }) {
  return (
    <div
      style={{
        flex: 1,
        overflow: "auto",
        background: "var(--paper)",
        padding: "40px 40px 60px",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
      }}
    >
      {children}
    </div>
  );
}
