/**
 * Polished video + audio player for the Preview surface.
 *
 * Replaces the bare `<video controls>` / `<audio controls>` browser
 * defaults that the user flagged as "too basic" (2026-06-16). Built on
 * `@vidstack/react`'s default-layout — gives us a Things 3 / Linear
 * grade player: poster auto-frame, large center play, segmented
 * scrubber with hover-preview, time + duration, mute + volume slider,
 * PiP, fullscreen, playback speed, captions slot (Phase 2).
 *
 * Themed via vidstack's CSS custom properties so it inherits the Slate
 * Console palette (cyan accent, slate paper, dark rail-style chrome).
 * Lazy-loaded from PreviewStage — only `.mp4` / `.mov` / `.mp3` / etc.
 * previews pay the bundle cost.
 */
import { MediaPlayer, MediaProvider, type MediaPlayerProps } from "@vidstack/react";
import {
  DefaultAudioLayout,
  defaultLayoutIcons,
  DefaultVideoLayout,
} from "@vidstack/react/player/layouts/default";

import "@vidstack/react/player/styles/default/theme.css";
import "@vidstack/react/player/styles/default/layouts/video.css";
import "@vidstack/react/player/styles/default/layouts/audio.css";
import "./media-player.css";

import { downloadUrl, type FileDto } from "../../api/client.ts";

interface Props {
  file: FileDto;
  kind: "video" | "audio";
}

export function DrivenMediaPlayer({ file, kind }: Props) {
  // vidstack reads media kind from the source MIME or the file
  // extension; passing it explicitly via `viewType` so the audio
  // and video layouts wire correctly without a metadata round-trip.
  const sharedProps: Partial<MediaPlayerProps> = {
    src: downloadUrl(file.id),
    title: file.name,
    viewType: kind,
    playsInline: true,
    // The src is on the user-content origin; vidstack defaults to
    // CORS-anonymous which fights our 302 redirect. Letting the
    // browser handle the navigation natively is fine for a player.
    load: "visible",
    storage: "cd-media-prefs",
  };

  if (kind === "audio") {
    return (
      <div className="cd-media-shell cd-media-shell--audio">
        <MediaPlayer {...sharedProps} aria-label={`Audio player: ${file.name}`}>
          <MediaProvider />
          <DefaultAudioLayout icons={defaultLayoutIcons} />
        </MediaPlayer>
      </div>
    );
  }

  return (
    <div className="cd-media-shell cd-media-shell--video">
      <MediaPlayer {...sharedProps} aria-label={`Video player: ${file.name}`}>
        <MediaProvider />
        <DefaultVideoLayout icons={defaultLayoutIcons} />
      </MediaPlayer>
    </div>
  );
}
