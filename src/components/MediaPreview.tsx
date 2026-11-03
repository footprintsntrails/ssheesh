// A photo or video in the preview in Files.

import { useEffect, useRef, useState } from "react";
import { Download } from "lucide-react";
import { backend, type OpenedMedia } from "../lib/backend";
import { bytes } from "../lib/format";
import { MAX_PICTURE, SVG, type MediaKind } from "../lib/media";
import { targetKey, toOpError, type TargetRef } from "../lib/types";
import { Button, Spinner } from "./ui";

/** So a picture with see-through parts can be told from what's behind it, on any theme. */
const SEE_THROUGH = { backgroundImage: "repeating-conic-gradient(rgb(128 128 128 / 0.14) 0 25%, transparent 0 50%)", backgroundSize: "18px 18px" };

const CANT_SHOW: Record<MediaKind, string> = {
  image: "This picture can't be shown here.",
  video: "This video can't be played here.",
};

/** `onDownload`: where the file's on a server, the way to look at it anyway. */
export function MediaPreview({ target, path, name, kind, onDownload }: { target: TargetRef; path: string; name: string; kind: MediaKind; onDownload?: () => void }) {
  const [opened, setOpened] = useState<OpenedMedia | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const [drawn, setDrawn] = useState<{ width: number; height: number } | null>(null);
  const [onFallback, setOnFallback] = useState(false);
  const openId = useRef<string | null>(null);
  const tk = targetKey(target);

  const close = () => {
    if (openId.current) backend.closeMedia(openId.current);
    openId.current = null;
  };

  useEffect(() => {
    let live = true;
    backend
      .openMedia(target, path)
      .then((file) => {
        if (!live) return backend.closeMedia(file.id);
        openId.current = file.id;
        setOpened(file);
        if (kind === "image" && file.size > MAX_PICTURE) {
          setProblem(`This picture is too big to show here (over ${bytes(MAX_PICTURE)}).`);
          close();
        }
      })
      .catch((e) => live && setProblem(toOpError(e).message));
    return () => {
      live = false;
      close();
    };
    // target is identified by its key.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tk, path, kind]);

  if (problem) {
    return (
      <div className="flex flex-1 flex-col items-center justify-center gap-3 p-6 text-center">
        <p className="text-[12.5px] text-fg-2">{problem}</p>
        {onDownload && (
          <Button size="sm" onClick={onDownload}>
            <Download size={13} /> Download
          </Button>
        )}
      </div>
    );
  }

  const failed = () => {
    if (!onFallback) return setOnFallback(true);
    setProblem(CANT_SHOW[kind]);
    // A server's connection isn't kept for what can't be shown.
    close();
  };
  const src = onFallback ? opened?.fallback : opened?.url;
  return (
    <>
      <div className="relative flex min-h-0 flex-1 items-center justify-center overflow-hidden" style={kind === "image" && drawn ? SEE_THROUGH : undefined}>
        {!drawn && (
          <div className="absolute inset-0 flex items-center justify-center text-fg-3">
            <Spinner />
          </div>
        )}
        {opened &&
          (kind === "image" ? (
            <img
              src={src}
              alt={name}
              draggable={false}
              onLoad={(e) => setDrawn({ width: e.currentTarget.naturalWidth, height: e.currentTarget.naturalHeight })}
              onError={failed}
              className="max-h-full max-w-full object-contain"
            />
          ) : (
            <video
              src={src}
              controls
              playsInline
              preload="metadata"
              onLoadedMetadata={(e) => setDrawn({ width: e.currentTarget.videoWidth, height: e.currentTarget.videoHeight })}
              onError={failed}
              className="max-h-full max-w-full"
            />
          ))}
      </div>
      {opened && (
        <p className="shrink-0 border-t border-line px-3 py-1 text-[11px] text-fg-3 tabular-nums">
          {/* A drawing has no size of its own: browsers tell the one it's shown at. */}
          {drawn && drawn.width > 0 && !SVG.test(name) ? `${drawn.width} × ${drawn.height} · ` : ""}
          {bytes(opened.size)}
        </p>
      )}
    </>
  );
}
