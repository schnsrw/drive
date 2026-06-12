/**
 * Theme toggle — Light / Dark / System.
 *
 * Cycle: light → dark → system → light. The choice persists to
 * `localStorage.theme` and writes `data-theme` on `<html>`. When
 * "system", the attribute is removed entirely so the
 * `@media (prefers-color-scheme: dark)` block in tokens.css takes
 * over — the OS preference drives the palette.
 *
 * Currently mounted in the Sidebar footer next to AvatarRow.
 * Without this UI the SPA silently followed OS preference with no
 * recourse — a regression users (rightly) hated.
 */
import { useEffect, useState } from "react";
import { Monitor, Moon, Sun } from "lucide-react";

type Theme = "light" | "dark" | "system";

const STORAGE_KEY = "theme";

function readStored(): Theme {
  if (typeof window === "undefined") return "system";
  try {
    const v = window.localStorage.getItem(STORAGE_KEY);
    if (v === "light" || v === "dark" || v === "system") return v;
  } catch {
    /* private mode — fall through */
  }
  return "system";
}

function resolveSystem(): "light" | "dark" {
  if (typeof window === "undefined") return "light";
  return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
}

export function ThemeToggle() {
  const [theme, setTheme] = useState<Theme>(readStored);

  useEffect(() => {
    if (theme === "system") {
      // Hand control back to the OS — `data-theme` removal lets the
      // `@media (prefers-color-scheme: dark)` rules in tokens.css
      // pick the palette from the system pref.
      document.documentElement.removeAttribute("data-theme");
    } else {
      // Explicit override — `data-theme="light" | "dark"` beats both
      // the unattributed system fallthrough AND the @media query.
      document.documentElement.setAttribute("data-theme", theme);
    }
    try {
      window.localStorage.setItem(STORAGE_KEY, theme);
    } catch {
      /* private mode — accept in-memory; the next load defaults */
    }
  }, [theme]);

  const cycle = () => {
    setTheme((t) => (t === "light" ? "dark" : t === "dark" ? "system" : "light"));
  };

  // Show the icon that reflects what the CURRENT theme looks like to
  // the eye — if system mode is currently rendering dark, show Moon,
  // not Monitor. That way the icon always matches what the user sees,
  // and Monitor only shows when the choice ITSELF is "system" AND the
  // user wants the cue that it's tracking the OS.
  const Icon = theme === "system" ? Monitor : theme === "dark" ? Moon : Sun;

  const labelMap: Record<Theme, string> = {
    light: "Light theme (click for dark)",
    dark: "Dark theme (click for system)",
    system: `System theme — currently ${resolveSystem()} (click for light)`,
  };

  return (
    <button
      type="button"
      onClick={cycle}
      aria-label={labelMap[theme]}
      title={labelMap[theme]}
      style={{
        // Sits inside the dark sidebar rail next to AvatarRow — uses
        // the rail-muted/rail-active palette so it reads on the dark
        // slate background. The toggle itself works the same in both
        // light + dark themes.
        display: "inline-flex",
        alignItems: "center",
        justifyContent: "center",
        width: 32,
        height: 32,
        borderRadius: 8,
        color: "var(--rail-muted)",
        background: "transparent",
        border: "1px solid transparent",
        cursor: "pointer",
        transition: "background 150ms, color 150ms, border-color 150ms",
        flexShrink: 0,
      }}
      onMouseOver={(e) => {
        e.currentTarget.style.background = "rgba(255,255,255,0.05)";
        e.currentTarget.style.color = "var(--rail-active-text)";
        e.currentTarget.style.borderColor = "var(--rail-line)";
      }}
      onMouseOut={(e) => {
        e.currentTarget.style.background = "transparent";
        e.currentTarget.style.color = "var(--rail-muted)";
        e.currentTarget.style.borderColor = "transparent";
      }}
    >
      <Icon size={16} strokeWidth={1.8} />
    </button>
  );
}
