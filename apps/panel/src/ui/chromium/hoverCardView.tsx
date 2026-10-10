// The hover card's face — one component, two carriers: the popup overlay
// webview (the real shell's widget, pointer-transparent host-side) and
// the browser demo's fixed-position stand-in. The paint here is
// Chromium's card geometry; the laws live in hoverCard.ts and the
// deltas in PROVENANCE.md.

import { useState } from "react";
import {
  GROUP_CARD_BULLET,
  HOVER_CARD_TITLE_MAX_LINES,
  groupCardFooterText,
  type HoverCardPayload,
} from "./hoverCard";
import "./hoverCard.css";

/** FadeLabelView's port: when the text changes, the PREVIOUS text stays
 *  on top and fades out while the new text shows underneath — the
 *  crossfade that rides the between-tab slide (fade_label_view.cc
 *  overlays the old label over the new one and animates its opacity).
 *  The line count is the card geometry's own: titles clamp at two
 *  lines, every other line stays single. */
export function FadeLabel({ text, lines = 1 }: { text: string; lines?: number }) {
  const [state, setState] = useState<{ current: string; prev: string | null }>({
    current: text,
    prev: null,
  });
  // Deriving state during render (React's adjust-state-on-prop-change
  // pattern): the old text must overlay the new in the SAME commit — a
  // post-commit effect would flash the new text for a frame.
  if (state.current !== text) {
    setState({ current: text, prev: state.current });
  }
  const clamped = Math.min(Math.max(lines, 1), HOVER_CARD_TITLE_MAX_LINES);
  return (
    <span className={`hc-label hc-lines-${clamped}`}>
      <span className="hc-text">{state.current}</span>
      {state.prev != null ? (
        <span
          className="hc-text hc-text-fading"
          onAnimationEnd={() => setState((cur) => ({ ...cur, prev: null }))}
        >
          {state.prev}
        </span>
      ) : null}
    </span>
  );
}

/** The card itself: a tab card is the title over the domain line; a
 *  group card is the composed header over up to five bullet members and
 *  the "+ N More" footer. `sliding` arms the position transition (the
 *  first paint must not glide in from nowhere); `fading` runs the
 *  200ms fade-out and reports back through `onFaded` — the carrier
 *  closes the widget when the fade lands, exactly upstream's
 *  FadeOut-then-close order. */
export function HoverCard({
  card,
  sliding = false,
  fading = false,
  onFaded,
}: {
  card: HoverCardPayload;
  sliding?: boolean;
  fading?: boolean;
  onFaded?: () => void;
}) {
  const group = card.kind === "group";
  return (
    <div
      className={[
        "hover-card",
        group ? "hc-group" : "",
        sliding ? "hover-card-sliding" : "",
        fading ? "hover-card-fading" : "",
      ].join(" ")}
      style={{ left: card.x, top: card.y }}
      role="presentation"
      aria-hidden
      onAnimationEnd={(e) => {
        if (e.animationName === "hc-fade-out") onFaded?.();
      }}
    >
      <div className={card.domain != null ? "hc-title hc-title-with-domain" : "hc-title"}>
        <FadeLabel text={card.title} lines={HOVER_CARD_TITLE_MAX_LINES} />
      </div>
      {card.kind === "tab" && card.domain != null ? (
        <div className="hc-domain">
          <FadeLabel text={card.domain} lines={1} />
        </div>
      ) : null}
      {group
        ? card.members.map((member) => (
            <div className="hc-member" key={member}>
              <FadeLabel text={GROUP_CARD_BULLET + member} lines={1} />
            </div>
          ))
        : null}
      {group && card.excess > 0 ? (
        <div className="hc-footer">
          <FadeLabel text={groupCardFooterText(card.excess)} lines={1} />
        </div>
      ) : null}
    </div>
  );
}
