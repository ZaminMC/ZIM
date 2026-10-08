# ADR-0030 — The motion language (§75)

**Status:** Accepted · **Date:** 2026-10-09

## Context

§75's design language says the interface should feel like an actual
desktop application, and its animations should be fast, intentional,
physically coherent, subtle where appropriate, and expressive during
major transitions. The panel already had motion tokens
(`--speed-fast`/`--speed-normal`, one easing curve) and a
reduced-motion guard that zeroed the two durations — but the audit for
this slice found the language unenforced at the edges:

- Three stylesheets carried hardcoded durations (`120ms`, `200ms`,
  `0.4s`) the tokens never reached, so a future "slow the whole
  language down" decision would silently miss them.
- The reduced-motion guard zeroed the tokens only — the infinite
  loops (spinner, status pulses) and the hardcoded durations kept
  moving forever for operators who asked the OS for less motion.
- One stylesheet borrowed a keyframes name defined in another CSS
  module; CSS modules scope keyframes per file, so the animation
  silently never ran (a `--radius`/`--surface-raised`-class token
  bug: the name resolved to nothing).
- The named §75 surfaces had no motion at all: tabs arrived with no
  entrance, the crash card appeared with no settle.

## Decision

**Three durations, one curve, compositor-only properties, and a hard
reduced-motion floor that no stylesheet can outrun.**

1. **The tokens are the whole vocabulary.** `--speed-fast` (120ms —
   hovers, focus, color), `--speed-normal` (200ms — meters, progress
   fills), `--speed-slow` (280ms — major entrances: modals, the
   crash card's neighbors). One easing curve (`--ease`,
   decelerating). Anything animated uses the tokens; the audit's
   stragglers were moved onto them.

2. **Entrances, not exits.** Tabs (§48) and the crash card (§62) get
   a two-pixel settle — opacity + `translateY(2px)` on the way in,
   fast, transform/opacity only so the strip and the alarm stay on
   the compositor. Exit animations need keep-mounted machinery the
   tab strip deliberately does not have; that stays reserved rather
   than faked (§82).

3. **Reduced motion is a floor, not a preference.** In addition to
   zeroing every duration token, a global guard kills durations and
   loop iterations for everything the tokens never reached:

   ```css
   @media (prefers-reduced-motion: reduce) {
     *, *::before, *::after {
       animation-duration: 0.01ms !important;
       animation-iteration-count: 1 !important;
       transition-duration: 0.01ms !important;
       scroll-behavior: auto !important;
     }
   }
   ```

   An animation lands on its end state immediately; nothing moves
   forever. The `!important` is deliberate and justified: this is an
   accessibility override, the one place a stylesheet must win.

4. **Dead tokens get aliases, not rewrites.** The 2026-10 refresh
   renamed the radius and raised-surface tokens and missed eight
   call sites (`var(--radius)` in the plugins/metrics/server views,
   `var(--surface-raised)` in inputs across six stylesheets), which
   have been resolving to 0 and transparent since. tokens.css
   defines `--radius: var(--radius-m)` and
   `--surface-raised: var(--surface-2)` so the old names mean what
   they always intended; new code uses the canonical scale.

5. **Deliberately instant.** The console's rows, the address bar's
   suggestions, and the strip's horizontal↔vertical swap do not
   animate: the first is a latency budget line (§76), the others are
   layout the operator did not ask to watch.

## Consequences

- A future retiming (or an OS-level reduced-motion request) reaches
  every animated surface by editing one file.
- The loops — spinner, starting/stopping pulses — keep their cadence
  for operators who did not ask for less motion, and run once for
  those who did; they remain state feedback, not decoration.
- The borrowed-keyframes bug is fixed where it lived: the lazy pane
  owns its `lazyPulse`.
