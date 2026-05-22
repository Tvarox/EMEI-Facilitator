"use client";

import { useEffect, useState } from "react";

type Spark = { x: number; y: number };

export default function DotGrid() {
  const [spark, setSpark] = useState<Spark | null>(null);

  useEffect(() => {
    const reduce = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    if (reduce) return;

    let mounted = true;

    const tick = () => {
      if (!mounted) return;
      const cols = Math.ceil(window.innerWidth / 64);
      const rows = Math.ceil(window.innerHeight / 64);
      const cx = Math.floor(Math.random() * Math.max(cols - 2, 1)) + 1;
      const cy = Math.floor(Math.random() * Math.max(rows - 2, 1)) + 1;
      setSpark({ x: cx * 64 - 4, y: cy * 64 - 4 });
      window.setTimeout(() => {
        if (mounted) setSpark(null);
      }, 600);
    };

    const id = window.setInterval(tick, 25000);
    const first = window.setTimeout(tick, 4000);

    return () => {
      mounted = false;
      window.clearInterval(id);
      window.clearTimeout(first);
    };
  }, []);

  return (
    <>
      <div className="dot-grid" aria-hidden />
      {spark && (
        <div
          className="dot-spark"
          aria-hidden
          style={{ position: "fixed", top: spark.y, left: spark.x, zIndex: 1 }}
        />
      )}
    </>
  );
}
