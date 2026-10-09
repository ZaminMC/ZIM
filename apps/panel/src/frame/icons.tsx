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

// -- Destination glyphs -------------------------------------------------------
// The favicon lane (upstream: every tab leads with its site's icon until
// favicons exist — the frame knows each destination's kind from its zim://
// URL, so the glyph stands in). Same 16-box stroke voice as the tools.

export function IconZim(props: SVGProps<SVGSVGElement>) {
  // The house mark: the Z bolt, the new-tab page's own glyph.
  return (
    <svg {...base(props)}>
      <path d="M4.2 3.5h7.6L4.2 12.5h7.6" />
    </svg>
  );
}

export function IconServer(props: SVGProps<SVGSVGElement>) {
  return (
    <svg {...base(props)}>
      <rect x="2.5" y="2.8" width="11" height="4.4" rx="1.2" />
      <rect x="2.5" y="8.8" width="11" height="4.4" rx="1.2" />
      <path d="M5 5h.01M5 11h.01" />
    </svg>
  );
}

export function IconTerminal(props: SVGProps<SVGSVGElement>) {
  return (
    <svg {...base(props)}>
      <rect x="2.5" y="3" width="11" height="10" rx="1.5" />
      <path d="m5 6.2 2.2 2.2L5 10.6M9.6 10.6h1.8" />
    </svg>
  );
}

export function IconGear(props: SVGProps<SVGSVGElement>) {
  // Settings as sliders: a gear's teeth die at 16px (they read as an
  // asterisk); three tracks with staggered knobs read as "adjust" at
  // any size.
  return (
    <svg {...base(props)}>
      <path d="M2.8 4.4h10.4M2.8 11.6h10.4" opacity={0.55} />
      <circle cx="10.4" cy="4.4" r="1.7" />
      <circle cx="5.6" cy="11.6" r="1.7" />
      <path d="M8 8h5.2M2.8 8h1.4" opacity={0.55} />
      <circle cx="6.9" cy="8" r="1.7" />
    </svg>
  );
}

export function IconClock(props: SVGProps<SVGSVGElement>) {
  return (
    <svg {...base(props)}>
      <circle cx="8" cy="8" r="5.4" />
      <path d="M8 5.2V8l2 1.4" />
    </svg>
  );
}

export function IconShield(props: SVGProps<SVGSVGElement>) {
  return (
    <svg {...base(props)}>
      <path d="M8 2.4l4.6 1.8v3.3c0 2.9-1.9 4.9-4.6 6.1-2.7-1.2-4.6-3.2-4.6-6.1V4.2L8 2.4Z" />
    </svg>
  );
}

export function IconInfo(props: SVGProps<SVGSVGElement>) {
  return (
    <svg {...base(props)}>
      <circle cx="8" cy="8" r="5.4" />
      <path d="M8 7.4v3.2M8 5.1v.01" />
    </svg>
  );
}

export function IconChat(props: SVGProps<SVGSVGElement>) {
  return (
    <svg {...base(props)}>
      <path d="M3.5 3.2h9a1.1 1.1 0 0 1 1.1 1.1v5a1.1 1.1 0 0 1-1.1 1.1H8.3L5 13.2v-2.8H3.5a1.1 1.1 0 0 1-1.1-1.1v-5a1.1 1.1 0 0 1 1.1-1.1Z" />
    </svg>
  );
}

export function IconApps(props: SVGProps<SVGSVGElement>) {
  return (
    <svg {...base(props)}>
      <rect x="2.8" y="2.8" width="4.6" height="4.6" rx="1" />
      <rect x="8.6" y="2.8" width="4.6" height="4.6" rx="1" />
      <rect x="2.8" y="8.6" width="4.6" height="4.6" rx="1" />
      <rect x="8.6" y="8.6" width="4.6" height="4.6" rx="1" />
    </svg>
  );
}

export function IconDownload(props: SVGProps<SVGSVGElement>) {
  return (
    <svg {...base(props)}>
      <path d="M8 2.6v6.6M5.2 6.4 8 9.2l2.8-2.8" />
      <path d="M3 11.2v1.2a1.2 1.2 0 0 0 1.2 1.2h7.6a1.2 1.2 0 0 0 1.2-1.2v-1.2" />
    </svg>
  );
}

export function IconGlobe(props: SVGProps<SVGSVGElement>) {
  return (
    <svg {...base(props)}>
      <circle cx="8" cy="8" r="5.4" />
      <path d="M2.6 8h10.8" />
      <path d="M8 2.6c-1.9 1.6-1.9 9.2 0 10.8M8 2.6c1.9 1.6 1.9 9.2 0 10.8" />
    </svg>
  );
}

export function IconChevLeft(props: SVGProps<SVGSVGElement>) {
  return (
    <svg {...base(props)}>
      <path d="M9.5 4 6 8l3.5 4" />
    </svg>
  );
}

export function IconChevRight(props: SVGProps<SVGSVGElement>) {
  return (
    <svg {...base(props)}>
      <path d="M6.5 4 10 8l-3.5 4" />
    </svg>
  );
}

export function IconDots(props: SVGProps<SVGSVGElement>) {
  return (
    <svg {...base(props)}>
      <circle cx="8" cy="3.2" r="1.1" fill="currentColor" stroke="none" />
      <circle cx="8" cy="8" r="1.1" fill="currentColor" stroke="none" />
      <circle cx="8" cy="12.8" r="1.1" fill="currentColor" stroke="none" />
    </svg>
  );
}

export function IconZoomIn(props: SVGProps<SVGSVGElement>) {
  return (
    <svg {...base(props)}>
      <circle cx="7" cy="7" r="4.4" />
      <path d="M10.4 10.4 13.5 13.5" />
      <path d="M7 5.2v3.6M5.2 7h3.6" />
    </svg>
  );
}

export function IconZoomOut(props: SVGProps<SVGSVGElement>) {
  return (
    <svg {...base(props)}>
      <circle cx="7" cy="7" r="4.4" />
      <path d="M10.4 10.4 13.5 13.5" />
      <path d="M5.2 7h3.6" />
    </svg>
  );
}

export function IconZoomReset(props: SVGProps<SVGSVGElement>) {
  return (
    <svg {...base(props)}>
      <path d="M13 8a5 5 0 1 1-1.5-3.6" />
      <path d="M13 2.8v2.4h-2.4" />
      <path d="M5.6 8h4.8" />
    </svg>
  );
}

export function IconDevTools(props: SVGProps<SVGSVGElement>) {
  return (
    <svg {...base(props)}>
      <path d="m5.5 5.5-2.5 2.5 2.5 2.5" />
      <path d="m10.5 5.5 2.5 2.5-2.5 2.5" />
      <path d="M9 3.5 7 12.5" />
    </svg>
  );
}

export function IconLogs(props: SVGProps<SVGSVGElement>) {
  return (
    <svg {...base(props)}>
      <path d="M3 4h10M3 8h10M3 12h6" />
    </svg>
  );
}

export function IconNewWindow(props: SVGProps<SVGSVGElement>) {
  return (
    <svg {...base(props)}>
      <rect x="2.6" y="4.6" width="10.8" height="8.8" rx="1.6" />
      <path d="M2.6 7.4h10.8" />
      <path d="M10 2.6h3.4V6" />
    </svg>
  );
}

export function IconCheck(props: SVGProps<SVGSVGElement>) {
  return (
    <svg {...base(props)}>
      <path d="m3.5 8.5 3 3 6-7" />
    </svg>
  );
}
