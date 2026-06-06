import { useEffect, useState } from "react";
import { Moon, Sun } from "lucide-react";

import { EmptyState } from "./components/EmptyState.tsx";
import { Logo } from "./components/Logo.tsx";

type Theme = "light" | "dark" | "system";

function resolveTheme(t: Theme): "light" | "dark" {
  if (t === "system") {
    return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
  }
  return t;
}

export function App() {
  const [theme, setTheme] = useState<Theme>(() => {
    return (localStorage.getItem("theme") as Theme) ?? "system";
  });

  useEffect(() => {
    const resolved = resolveTheme(theme);
    if (theme === "system") {
      document.documentElement.removeAttribute("data-theme");
    } else {
      document.documentElement.setAttribute("data-theme", resolved);
    }
    localStorage.setItem("theme", theme);
  }, [theme]);

  const cycleTheme = () => {
    setTheme((t) => (t === "light" ? "dark" : t === "dark" ? "system" : "light"));
  };

  const resolvedTheme = resolveTheme(theme);

  return (
    <div className="h-full w-full flex flex-col" style={{ background: "var(--bg-canvas)" }}>
      {/* Top bar — surface §3 minimal */}
      <header
        className="flex items-center justify-between px-6 sticky top-0 z-10"
        style={{
          height: "48px",
          background: "var(--bg-default)",
          borderBottom: "1px solid var(--border-default)",
        }}
      >
        <div className="flex items-center gap-2">
          <span style={{ color: "var(--fg-default)" }}>
            <Logo size={22} />
          </span>
          <span
            className="font-semibold tracking-tight"
            style={{ fontSize: "var(--text-md)", color: "var(--fg-default)" }}
          >
            Casual Drive
          </span>
          <span
            className="ml-3 px-2 py-0.5 rounded-xs"
            style={{
              fontSize: "var(--text-xs)",
              color: "var(--fg-muted)",
              background: "var(--bg-subtle)",
              borderRadius: "var(--radius-xs)",
            }}
          >
            spike #5
          </span>
        </div>
        <button
          type="button"
          onClick={cycleTheme}
          className="inline-flex items-center justify-center transition-colors"
          aria-label={`Theme: ${theme} (click to cycle)`}
          title={`Theme: ${theme}`}
          style={{
            width: "32px",
            height: "32px",
            borderRadius: "var(--radius-md)",
            color: "var(--fg-muted)",
            transitionDuration: "var(--dur-fast)",
            transitionTimingFunction: "var(--ease-out)",
          }}
          onMouseOver={(e) => (e.currentTarget.style.background = "var(--bg-hover)")}
          onMouseOut={(e) => (e.currentTarget.style.background = "transparent")}
        >
          {resolvedTheme === "dark" ? <Sun size={16} strokeWidth={2} /> : <Moon size={16} strokeWidth={2} />}
        </button>
      </header>

      {/* Main pane — empty state */}
      <main className="flex-1 flex items-center justify-center">
        <EmptyState />
      </main>
    </div>
  );
}
