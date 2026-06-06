/**
 * Procedural file thumbnails — the trick that makes an empty Drive look
 * lived-in. Each file type renders its own paper/sheet/play/folder visual
 * with a tint-tinted background. Per mockup-v2 + surface-v2 spec.
 */
import { Folder } from "lucide-react";

export type FileKind = "fold" | "doc" | "pdf" | "sheet" | "img" | "vid" | "aud" | "text" | "md" | "generic";

/** Source/code/data extensions rendered as monospaced text. */
const TEXT_EXTS = new Set([
  "txt", "log", "csv", "tsv",
  "json", "yaml", "yml", "toml", "ini", "conf",
  "xml", "html", "htm", "svg",
  "css", "scss", "sass", "less",
  "js", "jsx", "mjs", "cjs", "ts", "tsx",
  "py", "rb", "rs", "go", "java", "kt", "swift",
  "c", "h", "cpp", "hpp", "cc", "m", "mm",
  "php", "pl", "lua", "sql",
  "diff", "patch", "env", "lock",
]);

const GRADIENTS = [
  "linear-gradient(135deg,#f6d365,#fda085)",
  "linear-gradient(135deg,#a1c4fd,#c2e9fb)",
  "linear-gradient(135deg,#84fab0,#8fd3f4)",
  "linear-gradient(160deg,#30cfd0,#330867)",
  "linear-gradient(135deg,#667eea,#764ba2)",
  "linear-gradient(135deg,#ffecd2,#fcb69f)",
];

function gradient(seed: string) {
  let hash = 0;
  for (let i = 0; i < seed.length; i++) hash = (hash * 31 + seed.charCodeAt(i)) | 0;
  return GRADIENTS[Math.abs(hash) % GRADIENTS.length];
}

export function inferKind(name: string, contentType: string | null): FileKind {
  const ext = name.split(".").pop()?.toLowerCase() ?? "";
  const ct = contentType ?? "";
  if (ct === "__folder__") return "fold";
  if (ext === "md" || ext === "markdown" || ct === "text/markdown") return "md";
  if (
    ct.startsWith("image/") ||
    ["png", "jpg", "jpeg", "gif", "webp", "avif", "svg", "heic"].includes(ext)
  )
    return "img";
  if (ct.startsWith("video/") || ["mp4", "mov", "webm", "m4v"].includes(ext)) return "vid";
  if (ct.startsWith("audio/") || ["mp3", "wav", "ogg", "flac", "m4a", "aac"].includes(ext))
    return "aud";
  if (["xlsx", "ods"].includes(ext) || ct.includes("spreadsheet")) return "sheet";
  if (["docx"].includes(ext) || ct.includes("wordprocessingml")) return "doc";
  if (ext === "pdf" || ct === "application/pdf") return "pdf";
  if (TEXT_EXTS.has(ext) || ct.startsWith("text/")) return "text";
  return "generic";
}

export function FileThumb({
  name,
  kind,
  size = "tile",
  thumbnail,
}: {
  name: string;
  kind: FileKind;
  size?: "tile" | "small" | "big";
  /** Real preview data URI from the file row, when available. Used in
   * preference to the procedural gradient for image cards. */
  thumbnail?: string | null;
}) {
  const tint = `var(--tint-${kind === "generic" ? "doc" : kind})`;
  const padPct = size === "tile" ? "18% 20%" : size === "small" ? "12% 16%" : "0";

  if (kind === "img") {
    if (thumbnail) {
      return (
        <div
          aria-label={`Preview of ${name}`}
          role="img"
          style={{
            width: "100%",
            height: "100%",
            backgroundImage: `url(${JSON.stringify(thumbnail)})`,
            backgroundSize: "cover",
            backgroundPosition: "center",
            backgroundRepeat: "no-repeat",
          }}
        />
      );
    }
    return <div style={{ width: "100%", height: "100%", background: gradient(name) }} />;
  }

  if (kind === "vid") {
    return (
      <div
        style={{
          width: "100%",
          height: "100%",
          background: gradient(name),
          position: "relative",
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
        }}
      >
        <div
          style={{
            width: 34,
            height: 34,
            borderRadius: "50%",
            background: "rgba(255,255,255,.92)",
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            boxShadow: "0 3px 12px rgba(0,0,0,.3)",
          }}
        >
          <svg width={13} height={13} viewBox="0 0 24 24" fill="#1A1A1E" style={{ marginLeft: 2 }}>
            <path d="M8 5v14l11-7z" />
          </svg>
        </div>
      </div>
    );
  }

  if (kind === "fold") {
    return (
      <div
        style={{
          width: "100%",
          height: "100%",
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          background: tint,
        }}
      >
        <Folder
          size={size === "big" ? 80 : size === "tile" ? 46 : 18}
          strokeWidth={1.4}
          style={{ color: "var(--ic-fold)", opacity: 0.85 }}
        />
      </div>
    );
  }

  return (
    <div
      style={{
        width: "100%",
        height: "100%",
        background: tint,
        padding: padPct,
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        boxSizing: "border-box",
      }}
    >
      <Page kind={kind} />
    </div>
  );
}

function Page({ kind }: { kind: FileKind }) {
  if (kind === "sheet") {
    const cells = Array.from({ length: 20 });
    return (
      <div
        style={{
          background: "#fff",
          boxShadow: "0 1px 6px rgba(26,26,30,.10)",
          border: "1px solid rgba(26,26,30,.06)",
          borderRadius: 3,
          display: "grid",
          gridTemplateColumns: "repeat(4, 1fr)",
          gridTemplateRows: "repeat(5, 1fr)",
          overflow: "hidden",
          width: "100%",
          height: "100%",
        }}
      >
        {cells.map((_, i) => (
          <div
            key={i}
            style={{
              borderRight: "1px solid rgba(46,140,90,.18)",
              borderBottom: "1px solid rgba(46,140,90,.18)",
              background: i < 4 ? "rgba(46,140,90,.14)" : "transparent",
            }}
          />
        ))}
      </div>
    );
  }

  const isPdf = kind === "pdf";
  return (
    <div
      style={{
        background: "#fff",
        boxShadow: "0 1px 6px rgba(26,26,30,.10)",
        border: "1px solid rgba(26,26,30,.06)",
        borderRadius: 3,
        padding: "11% 13%",
        display: "flex",
        flexDirection: "column",
        gap: "6%",
        width: "100%",
        height: "100%",
        boxSizing: "border-box",
      }}
    >
      {isPdf ? (
        <div style={{ height: "11%", borderRadius: 2, background: "rgba(200,76,76,.85)", width: "40%" }} />
      ) : (
        <div style={{ height: "11%", width: "55%", borderRadius: 2, background: "rgba(26,26,30,.32)" }} />
      )}
      <div style={{ height: "6.5%", borderRadius: 2, background: "rgba(26,26,30,.10)" }} />
      <div style={{ height: "6.5%", width: "78%", borderRadius: 2, background: "rgba(26,26,30,.10)" }} />
      <div style={{ height: "6.5%", borderRadius: 2, background: "rgba(26,26,30,.10)" }} />
      <div style={{ height: "6.5%", width: "62%", borderRadius: 2, background: "rgba(26,26,30,.10)" }} />
    </div>
  );
}

/** Tiny inline icon used inline next to filenames (16 px). */
export function FileMiniIcon({ kind }: { kind: FileKind }) {
  const c = `var(--ic-${kind === "generic" ? "fold" : kind})`;
  const paths: Record<FileKind, string> = {
    fold: "M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z",
    doc: "M7 3h7l5 5v12a1 1 0 0 1-1 1H7a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1zM14 3v5h5",
    img: "M4 4h16v16H4zM9 9.5a1.5 1.5 0 1 1-3 0 1.5 1.5 0 0 1 3 0zM5 17l4-3.5 3 2 3-3 4 4.5",
    pdf: "M7 3h7l5 5v12a1 1 0 0 1-1 1H7a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1zM14 3v5h5",
    vid: "M3 5h18v14H3zM10 9.5l5 2.5-5 2.5z",
    aud: "M9 18V5l12-2v13M9 18a3 3 0 1 1-6 0 3 3 0 0 1 6 0zM21 16a3 3 0 1 1-6 0 3 3 0 0 1 6 0z",
    text: "M7 3h7l5 5v12a1 1 0 0 1-1 1H7a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1zM14 3v5h5M9 13h6M9 17h4",
    md: "M3 5h18v14H3zM6 16V8l3 4 3-4v8M16 8v8M14 13l2 3 2-3",
    sheet: "M7 3h7l5 5v12a1 1 0 0 1-1 1H7a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1zM14 3v5h5M9 13h6M12 11v8",
    generic: "M7 3h7l5 5v12a1 1 0 0 1-1 1H7a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1zM14 3v5h5",
  };
  return (
    <svg width={16} height={16} viewBox="0 0 24 24" fill="none" stroke={c} strokeWidth={1.6}>
      <path d={paths[kind]} />
    </svg>
  );
}
