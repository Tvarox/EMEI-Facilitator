"use client";

import { useEffect, useState } from "react";
import BootSequence from "./_components/BootSequence";
import BracketFrame from "./_components/BracketFrame";
import Chrome from "./_components/Chrome";
import Closing from "./_components/Closing";
import CustomCursor from "./_components/CustomCursor";
import DotGrid from "./_components/DotGrid";
import Hero from "./_components/Hero";
import Manifesto from "./_components/Manifesto";
import Primitives from "./_components/Primitives";
import Settlement from "./_components/Settlement";

export default function Page() {
  const [pageIndex, setPageIndex] = useState(0);

  useEffect(() => {
    const sections = Array.from(
      document.querySelectorAll<HTMLElement>("[data-act]"),
    );
    if (sections.length === 0) return;

    const obs = new IntersectionObserver(
      (entries) => {
        // Track entries currently intersecting and pick the one with the
        // largest intersection ratio.
        const visible = entries
          .filter((e) => e.isIntersecting)
          .sort((a, b) => b.intersectionRatio - a.intersectionRatio);
        if (visible[0]) {
          const idx = Number(visible[0].target.getAttribute("data-act") ?? 0);
          setPageIndex(idx);
        }
      },
      {
        threshold: [0.25, 0.5, 0.75],
      },
    );

    sections.forEach((s) => obs.observe(s));
    return () => obs.disconnect();
  }, []);

  return (
    <main style={{ position: "relative", zIndex: 2 }}>
      <DotGrid />
      <BracketFrame />
      <Chrome pageIndex={pageIndex} />
      <CustomCursor />
      <BootSequence />

      <Hero />
      <Manifesto />
      <Primitives />
      <Settlement />
      <Closing />
    </main>
  );
}
