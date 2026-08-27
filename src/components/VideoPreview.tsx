import { useEffect, useRef } from "react";
import type { FrameBytes, FrameListener } from "../lib/tauri";

interface VideoPreviewProps {
  subscribe: (listener: FrameListener) => () => void;
  running: boolean;
}

export function VideoPreview({ subscribe, running }: VideoPreviewProps) {
  const imageRef = useRef<HTMLImageElement>(null);

  useEffect(() => {
    if (!running) imageRef.current?.removeAttribute("src");

    let latestFrame: FrameBytes | null = null;
    let animationFrame: number | null = null;
    const objectUrls = new Set<string>();

    const paintLatestFrame = () => {
      animationFrame = null;
      const image = imageRef.current;
      if (!image || !latestFrame) return;

      const url = URL.createObjectURL(new Blob([latestFrame], { type: "image/jpeg" }));
      objectUrls.add(url);
      latestFrame = null;

      image.onload = () => {
        for (const existingUrl of objectUrls) {
          if (existingUrl !== url) {
            URL.revokeObjectURL(existingUrl);
            objectUrls.delete(existingUrl);
          }
        }
      };
      image.src = url;

      // Bound memory even when WebView2 skips load callbacks while frames arrive quickly.
      while (objectUrls.size > 4) {
        const staleUrl = objectUrls.values().next().value;
        if (!staleUrl || staleUrl === url) break;
        URL.revokeObjectURL(staleUrl);
        objectUrls.delete(staleUrl);
      }
    };

    const unsubscribe = subscribe((frame) => {
      latestFrame = frame;
      if (animationFrame === null) {
        animationFrame = requestAnimationFrame(paintLatestFrame);
      }
    });

    return () => {
      unsubscribe();
      if (animationFrame !== null) cancelAnimationFrame(animationFrame);
      for (const url of objectUrls) URL.revokeObjectURL(url);
      imageRef.current?.removeAttribute("src");
    };
  }, [running, subscribe]);

  return (
    <section className="preview-shell" aria-label="ShadowCast video preview">
      <div className="preview-grid" aria-hidden="true" />
      <img ref={imageRef} className="preview-image" alt="ShadowCast live video" />
      <div className={`preview-placeholder ${running ? "preview-placeholder--waiting" : ""}`}>
        <div className="signal-mark" aria-hidden="true">
          <span />
          <span />
          <span />
        </div>
        <p>{running ? "Waiting for video signal" : "ShadowCast is standing by"}</p>
        <span>{running ? "The first frame will appear here" : "Press Start to begin capture"}</span>
      </div>
      <div className="preview-badge">
        <i className={running ? "live-dot" : "idle-dot"} />
        {running ? "LIVE" : "OFFLINE"}
      </div>
      <div className="preview-label">GENKI SHADOWCAST · DIRECT CAPTURE</div>
    </section>
  );
}
