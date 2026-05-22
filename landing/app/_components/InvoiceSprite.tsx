"use client";

import { useEffect, useState } from "react";

const FRAMES = ["ISSUED", "PRESENTED", "PAID", ""] as const;

export default function InvoiceSprite() {
  const [i, setI] = useState(0);

  useEffect(() => {
    const reduce = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    if (reduce) return;
    const id = window.setInterval(() => setI((n) => (n + 1) % FRAMES.length), 800);
    return () => window.clearInterval(id);
  }, []);

  const stamp = FRAMES[i];

  return (
    <svg
      role="img"
      aria-label="Invoice cycling through ISSUED, PRESENTED, PAID."
      viewBox="0 0 360 360"
      width="100%"
      height="100%"
      style={{
        shapeRendering: "crispEdges",
        maxWidth: 420,
      }}
    >
      {/* ---- Isometric platform ---- */}
      <g stroke="var(--ink)" strokeWidth="2" fill="none">
        <polygon points="60,280 180,340 300,280 180,220" fill="var(--bg)" />
        <line x1="60" y1="280" x2="60" y2="290" />
        <line x1="300" y1="280" x2="300" y2="290" />
        <line x1="180" y1="340" x2="180" y2="350" />
        <polyline points="60,290 180,350 300,290" />
      </g>

      {/* ---- Accent shadow line on platform ---- */}
      <line
        x1="80"
        y1="296"
        x2="280"
        y2="296"
        stroke="var(--accent)"
        strokeWidth="2"
      />

      {/* ---- Document body (isometric, sitting on platform) ---- */}
      <g
        stroke="var(--ink)"
        strokeWidth="2"
        fill="var(--bg)"
        shapeRendering="crispEdges"
      >
        {/* top face */}
        <polygon points="100,180 180,220 260,180 180,140" />
      </g>

      {/* document side walls (depth to make it feel like a stack) */}
      <g stroke="var(--ink)" strokeWidth="2" fill="var(--bg)">
        <polygon points="100,180 100,200 180,240 180,220" />
        <polygon points="180,220 180,240 260,200 260,180" />
      </g>

      {/* halftone fill on the right wall for weight */}
      <defs>
        <pattern id="dots" x="0" y="0" width="6" height="6" patternUnits="userSpaceOnUse">
          <circle cx="1" cy="1" r="1" fill="var(--ink)" />
        </pattern>
        <clipPath id="rightWall">
          <polygon points="180,220 180,240 260,200 260,180" />
        </clipPath>
      </defs>
      <rect
        x="180"
        y="180"
        width="80"
        height="60"
        fill="url(#dots)"
        clipPath="url(#rightWall)"
        opacity="0.6"
      />

      {/* document text lines on the top face */}
      <g stroke="var(--ink)" strokeWidth="2" strokeLinecap="butt">
        <line x1="120" y1="174" x2="200" y2="214" />
        <line x1="140" y1="166" x2="220" y2="206" />
        <line x1="160" y1="158" x2="240" y2="198" />
      </g>

      {/* status stamp tile (only when stamp is non-empty) */}
      {stamp && (
        <g shapeRendering="crispEdges">
          <polygon
            points="190,170 250,200 250,222 190,192"
            fill="var(--accent)"
            stroke="var(--ink)"
            strokeWidth="2"
          />
          <text
            x="220"
            y="196"
            textAnchor="middle"
            fontFamily="var(--font-press-start), monospace"
            fontSize="9"
            fill="var(--bg)"
            transform="rotate(26 220 196)"
            letterSpacing="1"
          >
            {stamp}
          </text>
        </g>
      )}

      {/* mono frame index */}
      <text
        x="60"
        y="40"
        fontFamily="var(--font-plex-mono), ui-monospace, monospace"
        fontSize="12"
        fill="var(--ink)"
        letterSpacing="2"
      >
        FRAME {(i + 1).toString().padStart(2, "0")} / 04
      </text>

      {/* corner ticks */}
      <g stroke="var(--ink)" strokeWidth="2">
        <line x1="40" y1="60" x2="56" y2="60" />
        <line x1="40" y1="60" x2="40" y2="76" />
        <line x1="320" y1="60" x2="304" y2="60" />
        <line x1="320" y1="60" x2="320" y2="76" />
        <line x1="40" y1="320" x2="56" y2="320" />
        <line x1="40" y1="320" x2="40" y2="304" />
        <line x1="320" y1="320" x2="304" y2="320" />
        <line x1="320" y1="320" x2="320" y2="304" />
      </g>
    </svg>
  );
}
