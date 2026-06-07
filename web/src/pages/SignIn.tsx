import { useState } from "react";
import { Sparkles } from "lucide-react";

import { ApiError, DEMO_MODE } from "../api/client.ts";
import { useAuth } from "../auth/AuthContext.tsx";
import { Logo } from "../components/Logo.tsx";

const DEMO_USERNAME = "demo";
const DEMO_PASSWORD = "demo";

export function SignIn() {
  const { signIn } = useAuth();
  const [username, setUsername] = useState(DEMO_MODE ? DEMO_USERNAME : "");
  const [password, setPassword] = useState(DEMO_MODE ? DEMO_PASSWORD : "");
  const [capsOn, setCapsOn] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [shake, setShake] = useState(false);

  async function onSubmit(e: React.FormEvent) {
    e.preventDefault();
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      await signIn(username.trim(), password);
    } catch (err) {
      const msg =
        err instanceof ApiError
          ? err.status === 429
            ? "Too many attempts. Try again in 10 minutes."
            : "Wrong username or password."
          : "Couldn't reach the server.";
      setError(msg);
      setShake(true);
      setTimeout(() => setShake(false), 300);
    } finally {
      setBusy(false);
    }
  }

  const submitDisabled = busy || !password || !username.trim();

  return (
    <div
      style={{
        height: "100%",
        width: "100%",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        background: "var(--paper)",
      }}
    >
      <form
        onSubmit={onSubmit}
        style={{
          width: 360,
          padding: "32px 26px 26px",
          background: "var(--card)",
          border: "1px solid var(--line)",
          borderRadius: "var(--radius-xl)",
          boxShadow: "var(--shadow-soft, var(--shadow))",
          display: "flex",
          flexDirection: "column",
          gap: 16,
          animation: shake ? "cd-shake 300ms var(--ease)" : undefined,
        }}
      >
        <div style={{ display: "flex", flexDirection: "column", alignItems: "center", gap: 6 }}>
          <div style={{ color: "var(--ink)", marginBottom: 4 }}>
            <Logo size={36} />
          </div>
          <h1
            style={{
              margin: 0,
              fontFamily: "var(--font-display)",
              fontSize: "var(--text-2xl)",
              fontWeight: 500,
              letterSpacing: "var(--tracking-tight)",
              color: "var(--ink)",
            }}
          >
            Casual Drive
          </h1>
          <p style={{ margin: 0, fontSize: "var(--text-base)", color: "var(--muted)" }}>
            {DEMO_MODE ? "Demo build · sign in with the pre-filled credentials." : "Sign in to continue."}
          </p>
        </div>

        {DEMO_MODE && (
          <div
            style={{
              display: "flex",
              alignItems: "flex-start",
              gap: 11,
              padding: "11px 13px",
              background: "var(--accent-muted)",
              border: "1px solid rgba(200, 164, 92, 0.32)",
              borderRadius: 12,
              fontSize: "var(--text-xs)",
              color: "var(--ink-soft)",
              lineHeight: "var(--leading-normal)",
            }}
          >
            <Sparkles size={14} strokeWidth={1.8} style={{ color: "var(--accent)", flexShrink: 0, marginTop: 1 }} />
            <div>
              Username{" "}
              <code style={kbdStyle()}>{DEMO_USERNAME}</code>
              {" · "}
              Password{" "}
              <code style={kbdStyle()}>{DEMO_PASSWORD}</code>
              <div style={{ marginTop: 4, color: "var(--muted)" }}>
                Any credentials work — this build has no real auth.
              </div>
            </div>
          </div>
        )}

        <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
          <Input
            type="text"
            name="username"
            autoComplete="username"
            placeholder="Username"
            autoFocus
            disabled={busy}
            invalid={error !== null}
            value={username}
            onChange={(v) => setUsername(v)}
          />
          <Input
            type="password"
            name="password"
            autoComplete="current-password"
            placeholder="Password"
            disabled={busy}
            invalid={error !== null}
            value={password}
            onChange={(v) => setPassword(v)}
            onCapsLockChange={setCapsOn}
          />
          {capsOn && (
            <div
              role="status"
              style={{
                marginTop: 2,
                fontSize: "var(--text-xs)",
                color: "var(--warning)",
                display: "flex",
                alignItems: "center",
                gap: 6,
              }}
            >
              <span aria-hidden="true" style={{ fontSize: 10 }}>⇪</span>
              Caps Lock is on.
            </div>
          )}
        </div>

        {error && (
          <div
            role="alert"
            style={{
              marginTop: -8,
              fontSize: "var(--text-xs)",
              color: "var(--danger)",
              textAlign: "left",
            }}
          >
            {error}
          </div>
        )}

        <button
          type="submit"
          disabled={submitDisabled}
          style={{
            width: "100%",
            padding: "12px",
            fontFamily: "var(--font-sans)",
            fontSize: "var(--text-sm)",
            fontWeight: 500,
            color: "var(--paper)",
            background: submitDisabled ? "rgba(26,26,30,.35)" : "var(--ink)",
            border: "none",
            borderRadius: 12,
            cursor: submitDisabled ? "default" : "pointer",
            transition: "background 200ms var(--ease), transform 200ms",
          }}
          onMouseOver={(e) => {
            if (!submitDisabled) e.currentTarget.style.transform = "translateY(-1px)";
          }}
          onMouseOut={(e) => (e.currentTarget.style.transform = "")}
        >
          {busy ? "Signing in…" : "Sign in"}
        </button>
      </form>

      <style>
        {`
          @keyframes cd-shake {
            0%,100% { transform: translateX(0); }
            25%     { transform: translateX(-6px); }
            75%     { transform: translateX(6px); }
          }
          @media (prefers-reduced-motion: reduce) {
            form { animation: none !important; }
          }
        `}
      </style>
    </div>
  );
}

function kbdStyle(): React.CSSProperties {
  return {
    fontFamily: "var(--font-mono, ui-monospace, monospace)",
    background: "var(--card)",
    border: "1px solid var(--line)",
    borderRadius: 5,
    padding: "1px 6px",
    fontSize: 11,
    color: "var(--ink)",
  };
}

function Input({
  type,
  name,
  autoComplete,
  placeholder,
  autoFocus,
  disabled,
  invalid,
  value,
  onChange,
  onCapsLockChange,
}: {
  type: "text" | "password";
  name: string;
  autoComplete: string;
  placeholder: string;
  autoFocus?: boolean;
  disabled?: boolean;
  invalid?: boolean;
  value: string;
  onChange: (v: string) => void;
  /** Fires when the keyboard's Caps Lock modifier flips while this input
   * has focus. Used by the sign-in form to surface a one-line warning
   * under the password field — silent for other inputs. */
  onCapsLockChange?: (on: boolean) => void;
}) {
  return (
    <input
      type={type}
      name={name}
      autoFocus={autoFocus}
      autoComplete={autoComplete}
      placeholder={placeholder}
      disabled={disabled}
      aria-invalid={invalid || undefined}
      value={value}
      onChange={(e) => onChange(e.target.value)}
      onKeyDown={(e) => onCapsLockChange?.(e.getModifierState("CapsLock"))}
      onKeyUp={(e) => onCapsLockChange?.(e.getModifierState("CapsLock"))}
      style={{
        width: "100%",
        padding: "12px 14px",
        fontFamily: "var(--font-sans)",
        fontSize: "var(--text-base)",
        color: "var(--ink)",
        background: "var(--paper)",
        border: `1px solid ${invalid ? "var(--danger)" : "var(--line)"}`,
        borderRadius: 12,
        outline: "none",
        transition: "border-color 150ms, box-shadow 150ms",
      }}
      onFocus={(e) => {
        e.currentTarget.style.borderColor = invalid ? "var(--danger)" : "var(--line-strong)";
        e.currentTarget.style.boxShadow = "0 0 0 4px rgba(26,26,30,.04)";
      }}
      onBlur={(e) => {
        e.currentTarget.style.borderColor = invalid ? "var(--danger)" : "var(--line)";
        e.currentTarget.style.boxShadow = "";
        onCapsLockChange?.(false);
      }}
    />
  );
}
