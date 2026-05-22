export default function BracketFrame() {
  // Continuous frame inset 16px, broken at corners by 32px gaps.
  // Top edge: from x=48 to right=48; same logic on each side.
  return (
    <>
      <div
        className="frame-edge hidden md:block"
        style={{ top: 16, left: 48, right: 48, height: 2 }}
        aria-hidden
      />
      <div
        className="frame-edge hidden md:block"
        style={{ bottom: 16, left: 48, right: 48, height: 2 }}
        aria-hidden
      />
      <div
        className="frame-edge hidden md:block"
        style={{ left: 16, top: 48, bottom: 48, width: 2 }}
        aria-hidden
      />
      <div
        className="frame-edge hidden md:block"
        style={{ right: 16, top: 48, bottom: 48, width: 2 }}
        aria-hidden
      />

      {/* Mobile: simple full outline */}
      <div
        className="frame-edge md:hidden"
        style={{ top: 8, left: 8, right: 8, height: 2 }}
        aria-hidden
      />
      <div
        className="frame-edge md:hidden"
        style={{ bottom: 8, left: 8, right: 8, height: 2 }}
        aria-hidden
      />
      <div
        className="frame-edge md:hidden"
        style={{ left: 8, top: 8, bottom: 8, width: 2 }}
        aria-hidden
      />
      <div
        className="frame-edge md:hidden"
        style={{ right: 8, top: 8, bottom: 8, width: 2 }}
        aria-hidden
      />
    </>
  );
}
