import SectionLabel from "./SectionLabel";

export default function Manifesto() {
  return (
    <section
      id="act-2"
      data-act="1"
      className="relative min-h-screen flex items-center justify-center px-8 md:px-24 py-24"
    >
      <SectionLabel text="// ACT 02 :: PREMISE" />

      <div
        className="w-full"
        style={{
          border: "4px solid var(--accent)",
          padding: "clamp(40px, 8vw, 96px) clamp(24px, 6vw, 80px)",
          background: "var(--bg)",
          maxWidth: 1280,
        }}
      >
        <h2
          style={{
            fontFamily: "var(--font-press-start), monospace",
            fontSize: "clamp(22px, 5vw, 64px)",
            lineHeight: 1.25,
            color: "var(--ink)",
            textAlign: "center",
            margin: 0,
            letterSpacing: "0.01em",
          }}
        >
          THE INVOICE PREDATES THE COMPUTER.
        </h2>
      </div>
    </section>
  );
}
