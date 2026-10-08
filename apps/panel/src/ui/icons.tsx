// Inline icon set (16px stroke style, currentColor). Kept dependency-free:
// the panel's visual language needs only a dozen small marks, and every
// icon here is a plain <svg> — no icon-font, no package.

import type { CSSProperties } from "react";

interface IconProps {
  size?: number;
  className?: string;
  style?: CSSProperties;
}

function base(size: number | undefined, className: string | undefined, style: CSSProperties | undefined) {
  return {
    width: size ?? 16,
    height: size ?? 16,
    viewBox: "0 0 24 24",
    fill: "none",
    stroke: "currentColor",
    strokeWidth: 1.8,
    strokeLinecap: "round" as const,
    strokeLinejoin: "round" as const,
    className,
    style,
    "aria-hidden": true,
  };
}

export function IconDashboard(p: IconProps) {
  return (
    <svg {...base(p.size, p.className, p.style)}>
      <rect x="3" y="3" width="7.5" height="7.5" rx="1.5" />
      <rect x="13.5" y="3" width="7.5" height="7.5" rx="1.5" />
      <rect x="3" y="13.5" width="7.5" height="7.5" rx="1.5" />
      <rect x="13.5" y="13.5" width="7.5" height="7.5" rx="1.5" />
    </svg>
  );
}

export function IconServer(p: IconProps) {
  return (
    <svg {...base(p.size, p.className, p.style)}>
      <rect x="3" y="4" width="18" height="7" rx="2" />
      <rect x="3" y="13" width="18" height="7" rx="2" />
      <path d="M7 7.5h.01M7 16.5h.01" />
    </svg>
  );
}

export function IconPlus(p: IconProps) {
  return (
    <svg {...base(p.size, p.className, p.style)}>
      <path d="M12 5v14M5 12h14" />
    </svg>
  );
}

export function IconTerminal(p: IconProps) {
  return (
    <svg {...base(p.size, p.className, p.style)}>
      <path d="M4 17l6-5-6-5" />
      <path d="M12 19h8" />
    </svg>
  );
}

export function IconLogs(p: IconProps) {
  return (
    <svg {...base(p.size, p.className, p.style)}>
      <path d="M6 3h9l4 4v14a1 1 0 0 1-1 1H6a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1z" />
      <path d="M14 3v5h5M9 13h6M9 17h6" />
    </svg>
  );
}

export function IconFolder(p: IconProps) {
  return (
    <svg {...base(p.size, p.className, p.style)}>
      <path d="M3 7a2 2 0 0 1 2-2h4l2 2.5h8a2 2 0 0 1 2 2V18a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V7z" />
    </svg>
  );
}

export function IconPlayers(p: IconProps) {
  return (
    <svg {...base(p.size, p.className, p.style)}>
      <circle cx="9" cy="8" r="3.5" />
      <path d="M2.5 20c.8-3.2 3.4-5 6.5-5s5.7 1.8 6.5 5" />
      <path d="M16 5a3.5 3.5 0 0 1 0 7M18.5 15.4c1.6.7 2.7 2.1 3 4.6" />
    </svg>
  );
}

export function IconBackups(p: IconProps) {
  return (
    <svg {...base(p.size, p.className, p.style)}>
      <path d="M4 7h16v13a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V7z" />
      <path d="M3 7l2-3h14l2 3M10 11h4" />
    </svg>
  );
}

export function IconClock(p: IconProps) {
  return (
    <svg {...base(p.size, p.className, p.style)}>
      <circle cx="12" cy="12" r="8.5" />
      <path d="M12 7.5V12l3 2.5" />
    </svg>
  );
}

export function IconSparkles(p: IconProps) {
  return (
    <svg {...base(p.size, p.className, p.style)}>
      <path d="M12 4l1.6 4.2L18 9.8l-4.4 1.6L12 15.6l-1.6-4.2L6 9.8l4.4-1.6L12 4z" />
      <path d="M19 15l.8 2.1L22 18l-2.2.9L19 21l-.8-2.1L16 18l2.2-.9L19 15z" />
    </svg>
  );
}

export function IconBolt(p: IconProps) {
  return (
    <svg {...base(p.size, p.className, p.style)}>
      <path d="M13 2L4.5 13.5H11L9.5 22 19 10h-6.5L13 2z" />
    </svg>
  );
}

export function IconSearch(p: IconProps) {
  return (
    <svg {...base(p.size, p.className, p.style)}>
      <circle cx="11" cy="11" r="7" />
      <path d="M20 20l-3.5-3.5" />
    </svg>
  );
}

export function IconActivity(p: IconProps) {
  return (
    <svg {...base(p.size, p.className, p.style)}>
      <path d="M3 12h4l2.5-7 5 14 2.5-7h4" />
    </svg>
  );
}

export function IconWake(p: IconProps) {
  return (
    <svg {...base(p.size, p.className, p.style)}>
      <path d="M12 3v9" />
      <path d="M6.3 6.3a8 8 0 1 0 11.4 0" />
    </svg>
  );
}

export function IconPuzzle(p: IconProps) {
  return (
    <svg {...base(p.size, p.className, p.style)}>
      <path d="M10 3.5a1.75 1.75 0 0 1 3.5 0V5h3a1.5 1.5 0 0 1 1.5 1.5v3h1.5a1.75 1.75 0 0 1 0 3.5H18v4a1.5 1.5 0 0 1-1.5 1.5h-4V17a1.75 1.75 0 0 0-3.5 0v1.5h-4A1.5 1.5 0 0 1 3.5 17v-4H5a1.75 1.75 0 0 0 0-3.5H3.5v-3A1.5 1.5 0 0 1 5 5h5V3.5Z" />
    </svg>
  );
}

// --- browser chrome marks (ADR-0015) -----------------------------------------

export function IconArrowLeft(p: IconProps) {
  return (
    <svg {...base(p.size, p.className, p.style)}>
      <path d="M19 12H5" />
      <path d="M11 18l-6-6 6-6" />
    </svg>
  );
}

export function IconArrowRight(p: IconProps) {
  return (
    <svg {...base(p.size, p.className, p.style)}>
      <path d="M5 12h14" />
      <path d="M13 6l6 6-6 6" />
    </svg>
  );
}

export function IconReload(p: IconProps) {
  return (
    <svg {...base(p.size, p.className, p.style)}>
      <path d="M20 11a8 8 0 1 0-2.34 6.34" />
      <path d="M20 5v6h-6" />
    </svg>
  );
}

export function IconClose(p: IconProps) {
  return (
    <svg {...base(p.size, p.className, p.style)}>
      <path d="M6 6l12 12M18 6L6 18" />
    </svg>
  );
}

export function IconGear(p: IconProps) {
  return (
    <svg {...base(p.size, p.className, p.style)}>
      <circle cx="12" cy="12" r="3.2" />
      <path d="M12 2.8v2.6M12 18.6v2.6M2.8 12h2.6M18.6 12h2.6M5.5 5.5l1.9 1.9M16.6 16.6l1.9 1.9M18.5 5.5l-1.9 1.9M7.4 16.6l-1.9 1.9" />
    </svg>
  );
}

export function IconDots(p: IconProps) {
  return (
    <svg {...base(p.size, p.className, p.style)}>
      <path d="M12 5.5h.01M12 12h.01M12 18.5h.01" />
    </svg>
  );
}

export function IconNetwork(p: IconProps) {
  return (
    <svg {...base(p.size, p.className, p.style)}>
      <circle cx="12" cy="12" r="8.5" />
      <path d="M3.5 12h17M12 3.5c2.6 2.3 3.9 5.1 3.9 8.5s-1.3 6.2-3.9 8.5c-2.6-2.3-3.9-5.1-3.9-8.5s1.3-6.2 3.9-8.5Z" />
    </svg>
  );
}

export function IconRocket(p: IconProps) {
  return (
    <svg {...base(p.size, p.className, p.style)}>
      <path d="M12 15.5c5-3.2 7.2-7 7.2-11.7-4.7 0-8.5 2.2-11.7 7.2L5 13.6l5.4 5.4 1.6-3.5Z" />
      <path d="M8.6 15.4c-1.6.3-2.7 1.4-3.3 3.6 2.2-.6 3.3-1.7 3.6-3.3" />
      <circle cx="14.2" cy="9.8" r="1.3" />
    </svg>
  );
}

export function IconCopy(p: IconProps) {
  return (
    <svg {...base(p.size, p.className, p.style)}>
      <rect x="9" y="9" width="11" height="11" rx="2" />
      <path d="M5 15V6a2 2 0 0 1 2-2h9" />
    </svg>
  );
}

export function IconOpenInNew(p: IconProps) {
  return (
    <svg {...base(p.size, p.className, p.style)}>
      <path d="M14 4h6v6" />
      <path d="M20 4 11 13" />
      <path d="M18 13.5V18a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h4.5" />
    </svg>
  );
}
