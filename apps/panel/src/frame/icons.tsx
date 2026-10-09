// The frame's icon set — one hand, one voice. Stroke glyphs drawn on a
// 16-box (window controls on their own 10-box), currentColor throughout:
// the band tints them, they never assert a fill. Text glyphs (‹ ⟳ ☆)
// died in the 2026-10-09 pass — they rendered at the mercy of each
// platform's font and never matched.

import type { SVGProps } from "react";

const base = (props: SVGProps<SVGSVGElement>) => ({
  width: 16,
  height: 16,
  viewBox: "0 0 16 16",
  fill: "none",
  stroke: "currentColor",
  strokeWidth: 1.5,
  strokeLinecap: "round" as const,
  strokeLinejoin: "round" as const,
  "aria-hidden": true,
  ...props,
});

export function IconBack(props: SVGProps<SVGSVGElement>) {
  return (
    <svg {...base(props)}>
      <path d="M10.5 3.5 6 8l4.5 4.5" />
    </svg>
  );
}

export function IconForward(props: SVGProps<SVGSVGElement>) {
  return (
    <svg {...base(props)}>
      <path d="M5.5 3.5 10 8l-4.5 4.5" />
    </svg>
  );
}

export function IconReload(props: SVGProps<SVGSVGElement>) {
  return (
    <svg {...base(props)}>
      <path d="M13.5 8a5.5 5.5 0 1 1-1.61-3.89" />
      <path d="M13.55 1.9v2.4h-2.4" />
    </svg>
  );
}

export function IconStar(props: SVGProps<SVGSVGElement>) {
  return (
    <svg {...base(props)}>
      <path d="M8 2.2 9.75 5.74l3.93.57-2.84 2.77.67 3.91L8 11.16l-3.51 1.83.67-3.91-2.84-2.77 3.93-.57L8 2.2Z" />
    </svg>
  );
}

export function IconPlus(props: SVGProps<SVGSVGElement>) {
  return (
    <svg {...base(props)}>
      <path d="M8 3.5v9M3.5 8h9" />
    </svg>
  );
}

export function IconClose(props: SVGProps<SVGSVGElement>) {
  return (
    <svg {...base(props)} width={12} height={12} strokeWidth={1.3}>
      <path d="m4 4 8 8M12 4l-8 8" />
    </svg>
  );
}

export function IconSearch(props: SVGProps<SVGSVGElement>) {
  return (
    <svg {...base(props)} width={14} height={14}>
      <circle cx="7" cy="7" r="4.4" />
      <path d="m10.4 10.4 3.1 3.1" />
    </svg>
  );
}

export function IconMuted(props: SVGProps<SVGSVGElement>) {
  // The audio-state marker a muted tab wears (tab audio indicators):
  // a speaker with its strike, small enough to sit beside the title.
  return (
    <svg {...base(props)} width={12} height={12} strokeWidth={1.3}>
      <path d="M3 6.2v3.6h2.4L9 13V3L5.4 6.2H3Z" />
      <path d="m11 6 3 4M14 6l-3 4" />
    </svg>
  );
}

const win = (props: SVGProps<SVGSVGElement>) => ({
  width: 10,
  height: 10,
  viewBox: "0 0 10 10",
  fill: "none",
  stroke: "currentColor",
  strokeWidth: 1,
  strokeLinecap: "round" as const,
  "aria-hidden": true,
  ...props,
});

export function IconMinimize(props: SVGProps<SVGSVGElement>) {
  return (
    <svg {...win(props)}>
      <path d="M1.8 5.1h6.4" />
    </svg>
  );
}

export function IconMaximize(props: SVGProps<SVGSVGElement>) {
  return (
    <svg {...win(props)}>
      <rect x="2.3" y="2.3" width="5.4" height="5.4" rx="1" />
    </svg>
  );
}

export function IconWindowClose(props: SVGProps<SVGSVGElement>) {
  return (
    <svg {...win(props)} strokeWidth={1.1}>
      <path d="m2.5 2.5 5 5M7.5 2.5l-5 5" />
    </svg>
  );
}
