import SectionLabel from "./SectionLabel";

export default function Closing() {
  return (
    <section
      id="act-5"
      data-act="4"
      className="relative min-h-screen px-8 md:px-24 py-32 flex items-center"
    >
      <SectionLabel text="// ACT 05 :: SIGN-OFF" />

      <div className="w-full max-w-4xl">
        <div
          style={{
            fontFamily: "var(--font-vt323), ui-monospace, monospace",
            fontSize: "clamp(22px, 3vw, 36px)",
            lineHeight: 1.2,
            color: "var(--ink)",
            letterSpacing: "0.02em",
            marginBottom: 32,
          }}
        >
          STATUS: LIVE ON TESTNET. MAINNET PENDING.
        </div>

        <div
          className="flex flex-wrap items-center"
          style={{ gap: 24, marginBottom: 48 }}
        >
          <a
            href="https://github.com/Tvarox/EMEI-Contracts"
            target="_blank"
            rel="noreferrer"
            className="invert-hover"
            data-interactive="true"
          >
            [ GITHUB ]
          </a>
          <a
            href="https://github.com/Tvarox/EMEI-Contracts#readme"
            target="_blank"
            rel="noreferrer"
            className="invert-hover"
            data-interactive="true"
          >
            [ READ THE PROTOCOL ]
          </a>
        </div>

        <div
          style={{
            fontFamily: "var(--font-plex-mono), ui-monospace, monospace",
            fontSize: 13,
            fontStyle: "italic",
            color: "var(--ink)",
            letterSpacing: "0.04em",
            marginBottom: 12,
          }}
        >
          {"> Built for the agents that already exist."}
        </div>

        <span
          aria-hidden
          className="blink"
          style={{
            display: "inline-block",
            width: 12,
            height: 18,
            background: "var(--accent)",
          }}
        />
      </div>
    </section>
  );
}
