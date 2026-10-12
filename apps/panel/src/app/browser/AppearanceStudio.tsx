// The Appearance Studio (the Arix theme port's customization surface,
// 2026-10-12): a right sidebar the new tab's bottom-right icon opens,
// every control live, every change saved the moment it lands. This is
// the port of Arix's admin/arix editor panel (colors.blade.php,
// styling.blade.php — the customization options the founder asked to
// keep) moved to where a desktop panel's truth lives: the theme store
// (state/theme.ts), localStorage, and the host's cross-webview
// broadcast — so the frame, every tab, and every open menu re-theme in
// the same beat a slider moves.
//
// Design: Arix's sideEditor — a 320px glass sheet pinned right, its
// sections separated by hairlines, labels in the quiet gray300, the
// controls on the gray600 field with the gray500 border, the primary
// marking the live selection. The open/close verbs are TRANSFORM-only
// (compositor lanes): the sheet slides, the content never reflows.

import { useEffect, useState } from "react";
import {
  BACKGROUND_PRESETS,
  PRIMARY_SWATCHES,
  useTheme,
  type ThemeShape,
} from "../../state/theme";
import { useUi } from "../../state/ui";
import { IconClose, IconMoon, IconSun, IconZoomReset } from "../../ui/icons";
import styles from "./AppearanceStudio.module.css";

/** A labeled block — Arix's .input-field.hr law: section label above,
 *  hairline below, breathing room between. */
function Section({
  title,
  hint,
  children,
}: {
  title: string;
  hint?: string;
  children: React.ReactNode;
}) {
  return (
    <section className={styles.section}>
      <h3 className={styles.sectionTitle}>{title}</h3>
      {hint ? <p className={styles.sectionHint}>{hint}</p> : null}
      {children}
    </section>
  );
}

/** The mode segmented control — Arix's defaultMode (darkmode/lightmode). */
function ModeControl({ theme, onChange }: { theme: ThemeShape; onChange: (patch: Partial<ThemeShape>) => void }) {
  return (
    <div className={styles.segmented} role="radiogroup" aria-label="Color mode">
      <button
        type="button"
        role="radio"
        aria-checked={theme.mode === "dark"}
        className={`${styles.segment} ${theme.mode === "dark" ? styles.segmentActive : ""}`}
        onClick={() => onChange({ mode: "dark" })}
      >
        <IconMoon size={14} />
        Dark
      </button>
      <button
        type="button"
        role="radio"
        aria-checked={theme.mode === "light"}
        className={`${styles.segment} ${theme.mode === "light" ? styles.segmentActive : ""}`}
        onClick={() => onChange({ mode: "light" })}
      >
        <IconSun size={14} />
        Light
      </button>
    </div>
  );
}

/** The glass switch — Arix's backdrop flag, the port's namesake. */
function GlassControl({ theme, onChange }: { theme: ThemeShape; onChange: (patch: Partial<ThemeShape>) => void }) {
  return (
    <div className={styles.switchRow}>
      <div>
        <span className={styles.switchLabel}>Glass surfaces</span>
        <span className={styles.switchHint}>
          {theme.glass ? "Translucent panels with background blur" : "Solid panels, no translucency"}
        </span>
      </div>
      <button
        type="button"
        role="switch"
        aria-checked={theme.glass}
        aria-label="Glass surfaces"
        className={`${styles.switch} ${theme.glass ? styles.switchOn : ""}`}
        onClick={() => onChange({ glass: !theme.glass })}
      >
        <span className={styles.switchKnob} />
      </button>
    </div>
  );
}

/** A slider row — label left, value right, the input on the field. */
function SliderRow({
  label,
  value,
  min,
  max,
  step = 1,
  unit = "px",
  disabled,
  onInput,
}: {
  label: string;
  value: number;
  min: number;
  max: number;
  step?: number;
  unit?: string;
  disabled?: boolean;
  onInput: (value: number) => void;
}) {
  // The track's fill — the fraction of the range the thumb has walked,
  // stated as a CSS var the track's two-stop gradient consumes.
  const span = max - min;
  const fill = span > 0 ? ((value - min) / span) * 100 : 0;
  return (
    <label className={`${styles.sliderRow} ${disabled ? styles.sliderDisabled : ""}`}>
      <span className={styles.sliderHead}>
        <span className={styles.sliderLabel}>{label}</span>
        <span className={styles.sliderValue}>
          {value}
          {unit}
        </span>
      </span>
      <input
        type="range"
        className={styles.range}
        style={{ "--fill": `${fill}%` } as React.CSSProperties}
        min={min}
        max={max}
        step={step}
        value={value}
        disabled={disabled}
        onInput={(e) => onInput(Number((e.target as HTMLInputElement).value))}
      />
    </label>
  );
}

/** The accent grid — Arix's primary, the swatches plus a custom slot. */
function AccentControl({ theme, onChange }: { theme: ThemeShape; onChange: (patch: Partial<ThemeShape>) => void }) {
  const custom = !PRIMARY_SWATCHES.includes(theme.primary);
  return (
    <div className={styles.swatchGrid}>
      {PRIMARY_SWATCHES.map((color) => (
        <button
          key={color}
          type="button"
          className={`${styles.swatch} ${theme.primary === color ? styles.swatchActive : ""}`}
          style={{ background: color }}
          aria-label={`Accent ${color}`}
          aria-pressed={theme.primary === color}
          onClick={() => onChange({ primary: color })}
        />
      ))}
      <label
        className={`${styles.swatchCustom} ${custom ? styles.swatchActive : ""}`}
        title="Custom accent"
      >
        <span
          className={styles.swatchCustomDot}
          style={{ background: theme.primary }}
        />
        <input
          type="color"
          className={styles.colorInput}
          value={theme.primary}
          onChange={(e) => onChange({ primary: e.target.value })}
          aria-label="Custom accent color"
        />
      </label>
    </div>
  );
}

/** The background presets — Arix's backgroundImage, curated. The tiles'
 *  previews ride the theme's own mode (the light variants render when
 *  the mode is light — the exact image the ground would blur). */
function BackgroundControl({ theme, onChange }: { theme: ThemeShape; onChange: (patch: Partial<ThemeShape>) => void }) {
  return (
    <div className={styles.bgGrid} data-mode={theme.mode}>
      {BACKGROUND_PRESETS.map((preset) => (
        <button
          key={preset.id}
          type="button"
          className={`${styles.bgTile} ${theme.background === preset.id ? styles.bgTileActive : ""}`}
          onClick={() => onChange({ background: preset.id })}
          aria-pressed={theme.background === preset.id}
        >
          <span
            className={styles.bgPreview}
            data-bg={preset.id}
            aria-hidden
          />
          <span className={styles.bgLabel}>{preset.label}</span>
        </button>
      ))}
    </div>
  );
}

export function AppearanceStudio() {
  const open = useUi((s) => s.studioOpen);
  const setStudioOpen = useUi((s) => s.setStudioOpen);
  const theme = useTheme((s) => s.theme);
  const setTheme = useTheme((s) => s.setTheme);
  const resetTheme = useTheme((s) => s.resetTheme);
  const [resetNote, setResetNote] = useState(false);

  // Escape closes — the same keyboard contract every overlay in the
  // shell speaks.
  useEffect(() => {
    if (!open) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") setStudioOpen(false);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, setStudioOpen]);

  // The reset note breathes once and settles — no toast machinery for
  // one honest sentence.
  useEffect(() => {
    if (!resetNote) return;
    const dwell = window.setTimeout(() => setResetNote(false), 1800);
    return () => window.clearTimeout(dwell);
  }, [resetNote]);

  const onReset = () => {
    resetTheme();
    setResetNote(true);
  };

  return (
    <>
      {/* The scrim: a click outside closes, the way every side sheet in
          the shell behaves — dim, never blackout, so the live preview
          stays readable underneath. */}
      {open ? (
        <div
          className={styles.scrim}
          onClick={() => setStudioOpen(false)}
          aria-hidden
        />
      ) : null}
      <aside
        className={`arix-scope ${styles.studio} ${open ? styles.studioOpen : ""}`}
        aria-label="Appearance studio"
        aria-hidden={!open}
        {...{ inert: open ? undefined : "" }}
      >
        <header className={styles.head}>
          <div>
            <h2 className={styles.title}>Appearance</h2>
            <p className={styles.subtitle}>Live — every change saves itself.</p>
          </div>
          <button
            type="button"
            className={styles.closeBtn}
            onClick={() => setStudioOpen(false)}
            aria-label="Close appearance studio"
          >
            <IconClose size={16} />
          </button>
        </header>

        <div className={styles.body}>
          <Section title="Mode">
            <ModeControl theme={theme} onChange={setTheme} />
          </Section>

          <Section title="Glass" hint="Arix backdrop — translucency and blur behind panels.">
            <GlassControl theme={theme} onChange={setTheme} />
            <SliderRow
              label="Translucency"
              unit="%"
              min={40}
              max={100}
              step={1}
              value={theme.glassOpacity}
              disabled={!theme.glass}
              onInput={(value) => setTheme({ glassOpacity: value })}
            />
            <SliderRow
              label="Blur"
              min={0}
              max={40}
              step={1}
              value={theme.glassBlur}
              disabled={!theme.glass}
              onInput={(value) => setTheme({ glassBlur: value })}
            />
          </Section>

          <Section title="Accent">
            <AccentControl theme={theme} onChange={setTheme} />
          </Section>

          <Section title="Corners">
            <SliderRow
              label="Panels"
              min={0}
              max={24}
              value={theme.radiusBox}
              onInput={(value) => setTheme({ radiusBox: value })}
            />
            <SliderRow
              label="Inputs"
              min={0}
              max={16}
              value={theme.radiusInput}
              onInput={(value) => setTheme({ radiusInput: value })}
            />
          </Section>

          <Section title="Background" hint="The layer the glass blurs.">
            <BackgroundControl theme={theme} onChange={setTheme} />
          </Section>

          <div className={styles.footer}>
            <button type="button" className={styles.resetBtn} onClick={onReset}>
              <IconZoomReset size={14} />
              Reset to defaults
            </button>
            {resetNote ? (
              <span className={styles.resetNote} role="status">
                Back to the Arix defaults.
              </span>
            ) : null}
          </div>
        </div>
      </aside>
    </>
  );
}
