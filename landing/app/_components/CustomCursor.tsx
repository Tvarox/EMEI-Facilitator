"use client";

import { useEffect, useRef, useState } from "react";

export default function CustomCursor() {
  const ref = useRef<HTMLDivElement | null>(null);
  const [mode, setMode] = useState<"square" | "bracket">("square");
  const [enabled, setEnabled] = useState(true);

  useEffect(() => {
    const reduce = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    const coarse = window.matchMedia("(pointer: coarse)").matches;
    if (reduce || coarse) {
      setEnabled(false);
      return;
    }

    const el = ref.current;
    if (!el) return;

    let raf = 0;
    let tx = -100;
    let ty = -100;

    const onMove = (e: MouseEvent) => {
      tx = Math.round(e.clientX / 4) * 4;
      ty = Math.round(e.clientY / 4) * 4;
      const target = e.target as HTMLElement | null;
      const interactive = target?.closest(
        "a,button,[role='button'],[data-interactive='true']",
      );
      setMode(interactive ? "bracket" : "square");
      if (!raf) {
        raf = requestAnimationFrame(() => {
          el.style.transform = `translate3d(${tx}px, ${ty}px, 0)`;
          raf = 0;
        });
      }
    };

    const onLeave = () => {
      el.style.opacity = "0";
    };
    const onEnter = () => {
      el.style.opacity = "1";
    };

    window.addEventListener("mousemove", onMove);
    document.addEventListener("mouseleave", onLeave);
    document.addEventListener("mouseenter", onEnter);

    return () => {
      window.removeEventListener("mousemove", onMove);
      document.removeEventListener("mouseleave", onLeave);
      document.removeEventListener("mouseenter", onEnter);
      if (raf) cancelAnimationFrame(raf);
    };
  }, []);

  if (!enabled) return null;

  return (
    <div
      ref={ref}
      aria-hidden
      style={{
        position: "fixed",
        top: 0,
        left: 0,
        width: 8,
        height: 8,
        marginLeft: -4,
        marginTop: -4,
        zIndex: 60,
        pointerEvents: "none",
        transform: "translate3d(-100px,-100px,0)",
        transition: "none",
      }}
    >
      {mode === "square" ? (
        <div
          style={{
            width: 8,
            height: 8,
            background: "var(--accent)",
          }}
        />
      ) : (
        <div
          style={{
            position: "relative",
            width: 24,
            height: 24,
            marginLeft: -8,
            marginTop: -8,
            fontFamily: "var(--font-plex-mono), ui-monospace, monospace",
            color: "var(--accent)",
            fontSize: 18,
            lineHeight: "20px",
            display: "flex",
            justifyContent: "space-between",
            alignItems: "center",
          }}
        >
          <span>[</span>
          <span>]</span>
        </div>
      )}
    </div>
  );
}
