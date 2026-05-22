"use client";

import { useEffect, useRef, useState } from "react";
import SectionLabel from "./SectionLabel";

/* ---------------------------------------------------------------- */
/* Card 1 — Invoice: cycling status chips                           */
/* ---------------------------------------------------------------- */

const CHIPS = ["ISSUED", "PRESENTED", "PAID"] as const;

function InvoiceVisual() {
  const [active, setActive] = useState(0);

  useEffect(() => {
    const reduce = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    if (reduce) return;
    const id = window.setInterval(
      () => setActive((n) => (n + 1) % CHIPS.length),
      2000,
    );
    return () => window.clearInterval(id);
  }, []);

  return (
    <div
      className="flex flex-wrap items-center gap-2"
      style={{
        fontFamily: "var(--font-plex-mono), ui-monospace, monospace",
        fontSize: 12,
        letterSpacing: "0.12em",
      }}
    >
      {CHIPS.map((c, i) => (
        <span key={c} className="flex items-center gap-2">
          <span
            style={{
              border: "2px solid var(--ink)",
              padding: "6px 10px",
              background: i === active ? "var(--accent)" : "var(--bg)",
              color: i === active ? "var(--bg)" : "var(--ink)",
            }}
          >
            [ {c} ]
          </span>
          {i < CHIPS.length - 1 && (
            <span style={{ color: "var(--ink)" }}>{">"}</span>
          )}
        </span>
      ))}
    </div>
  );
}

/* ---------------------------------------------------------------- */
/* Card 2 — Mandate: depleting block bar                            */
/* ---------------------------------------------------------------- */

const BAR_TOTAL = 18;

function MandateVisual() {
  const [filled, setFilled] = useState(BAR_TOTAL);
  const ref = useRef<HTMLDivElement | null>(null);
  const startedRef = useRef(false);

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const reduce = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    if (reduce) {
      setFilled(7);
      return;
    }

    const obs = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          if (entry.isIntersecting && !startedRef.current) {
            startedRef.current = true;
            const target = 7;
            const stepsCount = BAR_TOTAL - target;
            let step = 0;
            const id = window.setInterval(() => {
              step += 1;
              setFilled(BAR_TOTAL - step);
              if (step >= stepsCount) window.clearInterval(id);
            }, 220);
          }
        }
      },
      { threshold: 0.4 },
    );
    obs.observe(el);
    return () => obs.disconnect();
  }, []);

  const cap = Math.round((filled / BAR_TOTAL) * 1000);

  return (
    <div ref={ref}>
      <div
        style={{
          fontFamily: "var(--font-plex-mono), ui-monospace, monospace",
          fontSize: 14,
          letterSpacing: "0.06em",
          color: "var(--ink)",
          whiteSpace: "nowrap",
          overflow: "hidden",
          textOverflow: "clip",
        }}
      >
        {Array.from({ length: BAR_TOTAL }).map((_, i) => (
          <span
            key={i}
            style={{
              color: i < filled ? "var(--accent)" : "var(--ink)",
            }}
          >
            {i < filled ? "█" : "░"}
          </span>
        ))}
      </div>
      <div
        style={{
          marginTop: 12,
          fontFamily: "var(--font-plex-mono), ui-monospace, monospace",
          fontSize: 11,
          letterSpacing: "0.16em",
          color: "var(--ink)",
        }}
      >
        CAP {cap} / 1000 &nbsp;&nbsp; NET 7 &nbsp;&nbsp; SCOPE: VENDOR_A
      </div>
    </div>
  );
}

/* ---------------------------------------------------------------- */
/* Card 3 — Reputation: gate                                         */
/* ---------------------------------------------------------------- */

function ReputationVisual() {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const reduce = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    if (reduce) {
      setOpen(true);
      return;
    }
    const obs = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          setOpen(entry.isIntersecting);
        }
      },
      { threshold: 0.4 },
    );
    obs.observe(el);
    return () => obs.disconnect();
  }, []);

  return (
    <div ref={ref}>
      <div
        style={{
          fontFamily: "var(--font-plex-mono), ui-monospace, monospace",
          fontSize: 16,
          letterSpacing: "0.04em",
          color: "var(--ink)",
          whiteSpace: "nowrap",
          overflow: "hidden",
        }}
      >
        [ A ]──────
        <span style={{ color: open ? "var(--accent)" : "var(--ink)" }}>
          {open ? "┤ ├" : "┤├"}
        </span>
        ──────[ B ]
      </div>
      <div
        style={{
          marginTop: 12,
          fontFamily: "var(--font-plex-mono), ui-monospace, monospace",
          fontSize: 11,
          letterSpacing: "0.16em",
          color: "var(--ink)",
        }}
      >
        SCORE 73 / 50 &nbsp;&nbsp; <span style={{ color: open ? "var(--accent)" : "var(--ink)" }}>{open ? "PASS" : "HOLD"}</span>
      </div>
    </div>
  );
}

/* ---------------------------------------------------------------- */
/* Card shell                                                       */
/* ---------------------------------------------------------------- */

function Card({
  index,
  label,
  title,
  copy,
  children,
}: {
  index: number;
  label: string;
  title: string;
  copy: string;
  children: React.ReactNode;
}) {
  const ref = useRef<HTMLDivElement | null>(null);
  const [shown, setShown] = useState(false);

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const obs = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          if (entry.isIntersecting) {
            window.setTimeout(() => setShown(true), index * 200);
            obs.disconnect();
          }
        }
      },
      { threshold: 0.2 },
    );
    obs.observe(el);
    return () => obs.disconnect();
  }, [index]);

  return (
    <div
      ref={ref}
      className={`reveal ${shown ? "in" : ""}`}
      style={{
        border: "2px solid var(--ink)",
        background: "var(--bg)",
        padding: 28,
        boxShadow: "6px 6px 0 0 var(--ink)",
        display: "flex",
        flexDirection: "column",
        gap: 16,
        minHeight: 320,
      }}
    >
      <div
        style={{
          fontFamily: "var(--font-plex-mono), ui-monospace, monospace",
          fontSize: 11,
          letterSpacing: "0.18em",
          color: "var(--ink)",
        }}
      >
        {label}
      </div>
      <div
        style={{
          fontFamily: "var(--font-vt323), ui-monospace, monospace",
          fontSize: 32,
          lineHeight: 1.05,
          color: "var(--ink)",
          letterSpacing: "0.02em",
        }}
      >
        {title}
      </div>
      <div
        style={{
          fontFamily: "var(--font-plex-mono), ui-monospace, monospace",
          fontSize: 13,
          lineHeight: 1.55,
          color: "var(--ink)",
          letterSpacing: "0.02em",
          maxWidth: 320,
        }}
      >
        {copy}
      </div>
      <div style={{ marginTop: "auto", paddingTop: 16 }}>{children}</div>
    </div>
  );
}

/* ---------------------------------------------------------------- */
/* Section                                                          */
/* ---------------------------------------------------------------- */

export default function Primitives() {
  return (
    <section
      id="act-3"
      data-act="2"
      className="relative min-h-screen px-8 md:px-24 py-24 md:py-32"
    >
      <SectionLabel text="// ACT 03 :: PRIMITIVES" />

      <div className="grid grid-cols-1 md:grid-cols-3 gap-6 md:gap-8">
        <Card
          index={0}
          label="[ 01 / OBJECT ]"
          title="INVOICE."
          copy="A lifecycle, not a document."
        >
          <InvoiceVisual />
        </Card>
        <Card
          index={1}
          label="[ 02 / PERMISSION ]"
          title="MANDATE."
          copy="Standing permission. Cap. Counterparty. Clock."
        >
          <MandateVisual />
        </Card>
        <Card
          index={2}
          label="[ 03 / GATE ]"
          title="REPUTATION."
          copy="Both sides clear before value moves."
        >
          <ReputationVisual />
        </Card>
      </div>
    </section>
  );
}
