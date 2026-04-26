import { layoutWithLines, prepareWithSegments, type LayoutLinesResult, type PreparedTextWithSegments } from "@chenglou/pretext";
import { useEffect, useMemo, useRef, useState } from "preact/hooks";

type PretextParagraphProps = {
  text: string;
  className?: string;
  lineHeight?: number;
};

type PreparedBlock = {
  prepared: PreparedTextWithSegments;
  layout: LayoutLinesResult | null;
};

function useMeasuredWidth<T extends HTMLElement>() {
  const ref = useRef<T | null>(null);
  const [width, setWidth] = useState(0);

  useEffect(() => {
    const node = ref.current;
    if (!node) return;
    const observer = new ResizeObserver((entries) => {
      const next = entries[0]?.contentRect.width ?? 0;
      setWidth(next);
    });
    observer.observe(node);
    setWidth(node.getBoundingClientRect().width);
    return () => observer.disconnect();
  }, []);

  return { ref, width };
}

export function PretextParagraph({ text, className, lineHeight = 22 }: PretextParagraphProps) {
  const { ref, width } = useMeasuredWidth<HTMLDivElement>();
  const font = '500 15px "IBM Plex Sans", sans-serif';

  const prepared = useMemo<PreparedBlock>(() => {
    const normalized = text.replace(/\r\n/g, "\n");
    const preparedText = prepareWithSegments(normalized, font, {
      whiteSpace: "pre-wrap",
      wordBreak: "normal",
      letterSpacing: 0,
    });
    if (width <= 0) {
      return { prepared: preparedText, layout: null };
    }
    return {
      prepared: preparedText,
      layout: layoutWithLines(preparedText, width, lineHeight),
    };
  }, [font, lineHeight, text, width]);

  return (
    <div ref={ref} className={className}>
      {prepared.layout?.lines?.length ? (
        prepared.layout.lines.map((line, index) => (
          <div key={`${line.start.segmentIndex}:${line.start.graphemeIndex}:${index}`} className="pretext-line">
            {line.text || "\u00A0"}
          </div>
        ))
      ) : (
        <div className="pretext-line">{text}</div>
      )}
    </div>
  );
}
