// The Arix appearance engine (the theme port, 2026-10-12): one store
// owns every customization knob the panel offers, applies each one to
// the document as a CSS custom property the moment it changes, and
// persists the whole shape under one localStorage key — the same
// contract Arix's config/arix.php holds server-side, moved client-side
// where a desktop panel's truth lives.
//
// THE TOKEN LAW (STYLE-GUIDE): this module never styles, it only
// mints variables — tokens.css remains the one place that maps them to
// surfaces, so a theme change is a variable change and every page,
// frame band, and popup follows without a single sheet being touched.
//
// CROSS-WEBVIEW LAW: every tab is its own webview, and the frame is
// another. localStorage is shared (one origin), so a boot reads the
// saved theme synchronously (the inline boot script in each HTML —
// before first paint, no flash), and a LIVE change rides the host's
// `shell://theme` broadcast so every open webview re-applies the same
// shape in the same beat the operator drags a slider.

import { create } from "zustand";

/** The persisted shape. Versioned by the key — a future field addition
 *  starts a new key, never a migration. */
export interface ThemeShape {
  /** "dark" | "light" — Arix's defaultMode. */
  mode: "dark" | "light";
  /** Arix's backdrop: glass surfaces (blur + translucency) on/off. */
  glass: boolean;
  /** Arix's backdropPercentage — how opaque the glass surfaces are
   *  (higher = more solid, lower = more see-through). 40–100 %. */
  glassOpacity: number;
  /** The blur radius behind glass surfaces, in px (Arix fixes 16). */
  glassBlur: number;
  /** Arix's primary — the one accent everything derives from. */
  primary: string;
  /** Arix's radiusBox — cards, menus, panels. px. */
  radiusBox: number;
  /** Arix's radiusInput — fields and small controls. px. */
  radiusInput: number;
  /** The background layer's preset ("none" = flat ground). */
  background: "aurora" | "mesh" | "ribbon" | "none";
}

export const THEME_KEY = "zim.theme.v1";

/** Arix's own defaults, verbatim (config/arix.php): dark ground, glass
 *  on, the indigo primary, 10px boxes, 7px inputs. The background is
 *  the port's own choice — Arix ships none; the aurora preset is the
 *  panel's premium default because glass needs something to blur. */
export const THEME_DEFAULTS: ThemeShape = {
  mode: "dark",
  glass: true,
  glassOpacity: 80,
  glassBlur: 16,
  primary: "#4a35cf",
  radiusBox: 10,
  radiusInput: 7,
  background: "aurora",
};

/** The curated primaries (Arix's color port): the theme's identity, the
 *  success/danger greens and reds, and two ZIM-context blues. The
 *  custom slot completes the freedom Arix gives. */
export const PRIMARY_SWATCHES: string[] = [
  "#4a35cf", // arix indigo (default)
  "#1a73e8", // chromium blue
  "#0f9d6e", // emerald
  "#8250df", // plum
  "#d0186a", // rose
  "#b06000", // amber
];

/** The background presets the studio offers — ids match the files in
 *  public/backgrounds. */
export const BACKGROUND_PRESETS: { id: ThemeShape["background"]; label: string }[] = [
  { id: "aurora", label: "Aurora" },
  { id: "mesh", label: "Mesh" },
  { id: "ribbon", label: "Ribbon" },
  { id: "none", label: "Flat" },
];

const BACKGROUNDS: Record<ThemeShape["background"], string | null> = {
  aurora: "/backgrounds/aurora",
  mesh: "/backgrounds/mesh",
  ribbon: "/backgrounds/ribbon",
  none: null,
};

/** The resolved image for a preset under a mode — Arix's own law
 *  (backgroundImage vs backgroundImageLight): the dark ground blurs a
 *  dark aurora, the light ground a pastel wash, never one image asked
 *  to serve both. */
function backgroundImage(shape: ThemeShape): string | null {
  const base = BACKGROUNDS[shape.background];
  if (base === null) return null;
  return shape.mode === "light" ? `${base}-light.jpg` : `${base}.jpg`;
}

/** Clamp + validate a raw value into the shape's own bounds — the
 *  studio's sliders can only mint legal themes, and a corrupted
 *  localStorage entry degrades to the nearest legal value, never to a
 *  broken sheet. */
export function normalizeTheme(raw: unknown): ThemeShape {
  const source = (raw ?? {}) as Partial<Record<keyof ThemeShape, unknown>>;
  const mode = source.mode === "light" ? "light" : "dark";
  const clamp = (value: unknown, min: number, max: number, fallback: number) => {
    const parsed = typeof value === "number" ? value : Number(value);
    if (!Number.isFinite(parsed)) return fallback;
    return Math.min(max, Math.max(min, parsed));
  };
  const hex = (value: unknown, fallback: string) =>
    typeof value === "string" && /^#[0-9a-fA-F]{6}$/.test(value) ? value.toLowerCase() : fallback;
  const background = BACKGROUND_PRESETS.some((p) => p.id === source.background)
    ? (source.background as ThemeShape["background"])
    : THEME_DEFAULTS.background;
  return {
    mode,
    glass: typeof source.glass === "boolean" ? source.glass : THEME_DEFAULTS.glass,
    glassOpacity: clamp(source.glassOpacity, 40, 100, THEME_DEFAULTS.glassOpacity),
    glassBlur: clamp(source.glassBlur, 0, 40, THEME_DEFAULTS.glassBlur),
    primary: hex(source.primary, THEME_DEFAULTS.primary),
    radiusBox: clamp(source.radiusBox, 0, 24, THEME_DEFAULTS.radiusBox),
    radiusInput: clamp(source.radiusInput, 0, 16, THEME_DEFAULTS.radiusInput),
    background,
  };
}

/** Read the saved theme off localStorage (the boot script already
 *  applied it to the document; this re-reads for the store's truth). */
export function readSavedTheme(): ThemeShape {
  try {
    const raw = window.localStorage.getItem(THEME_KEY);
    if (!raw) return { ...THEME_DEFAULTS };
    return normalizeTheme(JSON.parse(raw));
  } catch {
    return { ...THEME_DEFAULTS };
  }
}

/** Apply one theme shape to a document — the ONLY writer of the
 *  customization variables. Called at boot (after the inline script's
 *  early pass), on every change, and on every cross-webview broadcast. */
export function applyTheme(theme: ThemeShape, target: Document = document): void {
  const root = target.documentElement;
  root.dataset.mode = theme.mode;
  root.dataset.glass = theme.glass ? "on" : "off";
  root.dataset.background = theme.background;
  root.style.setProperty("--primary", theme.primary);
  root.style.setProperty("--glass-opacity", `${theme.glassOpacity}%`);
  root.style.setProperty("--glass-blur", `${theme.glassBlur}px`);
  root.style.setProperty("--radius-box", `${theme.radiusBox}px`);
  root.style.setProperty("--radius-input", `${theme.radiusInput}px`);
  const image = backgroundImage(theme);
  root.style.setProperty("--background-image", image ? `url("${image}")` : "none");
  // The boot ground — the html canvas color a webview paints before any
  // content exists (the inline script sets the same var from the same
  // key, so first paint and every paint agree).
  root.style.setProperty("--boot-ground", theme.mode === "dark" ? "#0b0d2a" : "#f0f1f5");
}

interface ThemeState {
  theme: ThemeShape;
  /** True once this webview has attached the broadcast listener. */
  listening: boolean;
  setTheme: (patch: Partial<ThemeShape>) => void;
  resetTheme: () => void;
  /** Apply a broadcast shape (from the host event) without re-saving. */
  adoptBroadcast: (shape: ThemeShape) => void;
  attachBroadcast: () => void;
}

export const useTheme = create<ThemeState>()((set, get) => ({
  theme: readSavedTheme(),
  listening: false,

  setTheme: (patch) => {
    const next = normalizeTheme({ ...get().theme, ...patch });
    set({ theme: next });
    applyTheme(next);
    try {
      window.localStorage.setItem(THEME_KEY, JSON.stringify(next));
    } catch {
      // A storage failure (private mode, quota) only costs persistence —
      // the live document already carries the change.
    }
    void broadcastTheme(next);
  },

  resetTheme: () => get().setTheme({ ...THEME_DEFAULTS }),

  adoptBroadcast: (shape) => {
    set({ theme: shape });
    applyTheme(shape);
    try {
      window.localStorage.setItem(THEME_KEY, JSON.stringify(shape));
    } catch {
      // Same law as setTheme: the live application is the product.
    }
  },

  attachBroadcast: () => {
    if (get().listening) return;
    set({ listening: true });
    // The frame's own broadcast rides a DOM event (frameIpc forwards
    // host events); the tab documents listen on the host lane directly
    // when they run under Tauri, and fall back to storage events in a
    // plain browser (the dev bridge — same origin, same localStorage).
    window.addEventListener("zamin:theme-broadcast", ((event: CustomEvent) => {
      get().adoptBroadcast(normalizeTheme((event as CustomEvent).detail));
    }) as EventListener);
    window.addEventListener("storage", (event) => {
      if (event.key !== THEME_KEY || event.newValue === null) return;
      try {
        get().adoptBroadcast(normalizeTheme(JSON.parse(event.newValue)));
      } catch {
        // A malformed broadcast from a sibling degrades to silence —
        // this webview's own theme stands.
      }
    });
    // The host lane: attached lazily; in a plain browser (the dev bridge)
    // there is no host and the lane simply never opens — storage events
    // carry the change there.
    void import("../integration/tauri")
      .then(({ listenHost }) =>
        listenHost<ThemeShape>("shell://theme", (shape) => {
          get().adoptBroadcast(normalizeTheme(shape));
        }),
      )
      .catch(() => {
        // No host in this document — the storage fallback above is the lane.
      });
  },
}));

/** Tell the host a theme changed — it re-broadcasts to every webview
 *  (tabs, frame, popups) so the whole shell moves in one beat. The
 *  command is best-effort: in a plain browser there is no host, and
 *  the storage-event fallback keeps the dev bridge honest. */
async function broadcastTheme(theme: ThemeShape): Promise<void> {
  try {
    const { invokeHost } = await import("../integration/tauri");
    await invokeHost("shell_theme_broadcast", { theme });
  } catch {
    // No host (dev bridge) — storage events carry the change.
  }
}
