import { useRef } from "react";
import { grip } from "../../audio/pluginBridge.ts";

/** The plugin window's corner: a host can't drag a plugin's border (docs/plugin.md, "Window size"). */
export function ResizeGrip() {
  const drag = useRef<{ raf: number } | null>(null);
  const end = () => {
    if (!drag.current) return;
    cancelAnimationFrame(drag.current.raf);
    drag.current = null;
    grip("end");
  };
  return (
    <div
      className="resize-grip"
      data-layout="overlay"
      role="separator"
      aria-label="Resize the window. Double-click for the first size."
      title="Drag to resize. Double-click for the first size."
      onDoubleClick={() => grip("reset")}
      onPointerDown={(e) => {
        e.currentTarget.setPointerCapture(e.pointerId);
        e.preventDefault();
        drag.current = { raf: 0 };
        grip("begin");
      }}
      onPointerMove={() => {
        const d = drag.current;
        if (!d || d.raf) return;
        // at most once a frame: each one makes the host lay its window out again
        d.raf = requestAnimationFrame(() => {
          d.raf = 0;
          if (drag.current === d) grip("move");
        });
      }}
      onPointerUp={end}
      onPointerCancel={end}
    >
      <svg viewBox="0 0 16 16" width="16" height="16" aria-hidden="true">
        <path d="M15 5 5 15M15 9 9 15M15 13 13 15" fill="none" stroke="currentColor" strokeWidth="1" />
      </svg>
    </div>
  );
}
