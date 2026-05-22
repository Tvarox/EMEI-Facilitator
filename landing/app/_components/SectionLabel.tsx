export default function SectionLabel({ text }: { text: string }) {
  return (
    <div className="section-label" aria-hidden>
      {text}
    </div>
  );
}
