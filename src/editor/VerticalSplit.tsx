import { useRef, useState, type CSSProperties, type ReactNode, type RefObject } from "react";
import { PanelDivider } from "./PanelDivider";

type Props = {
  className: string;
  preference: string;
  label: string;
  controls: string;
  initial: number;
  minimum: number;
  maximum: number;
  children: ReactNode;
  layoutRef?: RefObject<HTMLDivElement | null>;
  style?: CSSProperties;
};

export function VerticalSplit({ children, className, label, controls, preference, minimum, maximum, initial, layoutRef, style }: Props) {
  const [height, setHeight] = useState(() => {
    try {
      const value = Number(localStorage.getItem(preference));
      if (value >= minimum && value <= maximum) return value;
    } catch { /* Use the default if local preferences are unavailable. */ }
    return initial;
  });
  const localRef = useRef<HTMLDivElement>(null);
  const grid = layoutRef ?? localRef;
  return <div ref={grid} className={className} style={{ ...style, "--lower-height": height + "%" } as CSSProperties}>
    {children}
    <PanelDivider className="height-divider" axis="y" label={label} controls={controls}
      value={height} minimum={minimum} maximum={maximum} perPixel={() => {
        const el = grid.current!;
        const css = getComputedStyle(el);
        return -100 / (el.clientHeight - parseFloat(css.paddingTop) - parseFloat(css.paddingBottom));
      }} onResize={(value, commit) => {
        setHeight(value);
        if (commit) try { localStorage.setItem(preference, String(value)); } catch { /* Keep the live layout. */ }
      }} />
  </div>;
}
