// Client-side thumbnail generator. Spec: docs/ux/07-preview-surface.md
// + pipeline §5.2. Produces a small data URI for image uploads which the
// server stores on the file row and surfaces back in list responses.
//
// Browser-only — uses <canvas> + URL.createObjectURL. Returns null when
// the file isn't an image, the browser refuses to decode it, or the
// output blows past the size cap.

/** Target square dimension in CSS pixels. 192 covers both list-row
 * (30 px) and grid-card (130 px) at 2× DPR. */
const TARGET = 192;

/** Hard cap matching the server's THUMBNAIL_MAX_BYTES. Anything bigger
 * is dropped server-side anyway. */
const MAX_BYTES = 64 * 1024;

const SUPPORTED = /^image\/(png|jpe?g|gif|webp|avif|bmp)$/;

/** Returns a `data:image/*;base64,…` URI or `null` if not applicable. */
export async function generateThumbnail(file: File): Promise<string | null> {
  if (typeof window === "undefined") return null;
  if (!SUPPORTED.test(file.type)) return null;

  let bitmap: ImageBitmap | null = null;
  let url: string | null = null;

  try {
    if (typeof createImageBitmap === "function") {
      bitmap = await createImageBitmap(file);
    } else {
      url = URL.createObjectURL(file);
      await loadImage(url);
    }
  } catch {
    if (url) URL.revokeObjectURL(url);
    return null;
  }

  const w = bitmap?.width ?? 0;
  const h = bitmap?.height ?? 0;
  if (w === 0 || h === 0) {
    if (url) URL.revokeObjectURL(url);
    return null;
  }

  const scale = Math.min(TARGET / w, TARGET / h, 1); // never upscale
  const outW = Math.max(1, Math.round(w * scale));
  const outH = Math.max(1, Math.round(h * scale));

  const canvas = document.createElement("canvas");
  canvas.width = outW;
  canvas.height = outH;
  const ctx = canvas.getContext("2d");
  if (!ctx) {
    if (url) URL.revokeObjectURL(url);
    return null;
  }
  ctx.imageSmoothingQuality = "high";
  if (bitmap) {
    ctx.drawImage(bitmap, 0, 0, outW, outH);
  } else if (url) {
    const img = await loadImage(url);
    ctx.drawImage(img, 0, 0, outW, outH);
  }
  if (url) URL.revokeObjectURL(url);

  // Try WebP first (best ratio at this size); fall back to JPEG, then PNG.
  const candidates = [
    () => canvas.toDataURL("image/webp", 0.82),
    () => canvas.toDataURL("image/jpeg", 0.82),
    () => canvas.toDataURL("image/png"),
  ];
  for (const make of candidates) {
    let uri: string;
    try {
      uri = make();
    } catch {
      continue;
    }
    if (uri && uri.length <= MAX_BYTES && uri.startsWith("data:image/")) return uri;
  }
  return null;
}

function loadImage(src: string): Promise<HTMLImageElement> {
  return new Promise((resolve, reject) => {
    const img = new Image();
    img.onload = () => resolve(img);
    img.onerror = () => reject(new Error("image decode failed"));
    img.src = src;
  });
}
