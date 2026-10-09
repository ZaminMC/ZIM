// The frame's layout law — ported from Chromium, not approximated.
//
// Every constant cites its upstream file (chromium/chromium @ main,
// mirrored in docs/browser-shell/chromium-ref/). The porting spec is
// docs/browser-shell/chromium-porting-spec.md; the doctrine (BSD-3,
// attribution, no branding) is ADR-0033.

/// `kTabWidth = 232` — chrome/browser/ui/tabs/tab_style.cc. The standard
/// tab's CONTENT width; the full slot adds the two bottom corner
/// extensions (`GetStandardWidth` = 232 + 2×12).
pub const TAB_WIDTH: f32 = 232.0;
/// `GetTopCornerRadius() = 10` — tab_style.cc. Lives in the frame's CSS
/// (border-radius) — the model keeps the documented constant.
#[allow(dead_code)]
pub const CORNER_TOP: f32 = 10.0;
/// `GetBottomCornerRadius() = 12` — tab_style.cc.
pub const CORNER_BOTTOM: f32 = 12.0;
/// `kTabHeight = 34 + kTabstripToolbarOverlap(1)` — layout_constants.cc.
pub const TAB_HEIGHT: f32 = 35.0;
/// `kTabStripPadding = 6` — layout_constants.cc.
pub const STRIP_PADDING: f32 = 6.0;
/// `kTabPinnedContentWidth = 24` — tab_style.cc GetPinnedWidth.
pub const PINNED_CONTENT: f32 = 24.0;
/// `kInteriorWidth = 16` — tab_style.cc GetMinimumInactiveWidth.
pub const MIN_INACTIVE_INTERIOR: f32 = 16.0;
/// `kSeparatorThickness = 2` — tab_style.cc.
pub const SEPARATOR_W: f32 = 2.0;
/// `kSeparatorHeight = 20` (touch_ui false) — layout_constants.cc. The
/// view sizes separators in CSS; the model keeps the documented constant.
#[allow(dead_code)]
pub const SEPARATOR_H: f32 = 20.0;
/// Separator horizontal margin — `kSeparatorHorizontalMargin = 2` (both
/// sides), tab_style.cc.
pub const SEPARATOR_MARGIN: f32 = 2.0;
/// `kTabCloseButtonSize = 16` — layout_constants.cc.
pub const CLOSE_BUTTON: f32 = 16.0;
/// `kTabHorizontalPadding = 8` — layout_constants.cc (this build's
/// contents insets; divergence noted in the porting spec).
pub const CONTENT_INSET_X: f32 = 8.0;
/// The new-tab button's slot width (upstream derives it from the border;
/// this build reserves 36 DIP — porting-spec divergence note).
pub const NEW_TAB_BUTTON_W: f32 = 36.0;
/// The Windows caption area the strip must never slide under: three
/// 46 DIP buttons (minimize, maximize, close), the width the frame's
/// window controls occupy (frame.css .window-controls button). Upstream
/// reserves the caption space through the frame layout
/// (tab_strip.cc buys out the caption buttons' rect); this build states
/// the reservation as a constant — porting-spec divergence note.
pub const WINDOW_CONTROLS_W: f32 = 138.0;
/// `kLocationBarHeight = 34` — layout_constants.cc.
pub const LOCATION_BAR_H: f32 = 34.0;
/// Toolbar vertical padding around the location bar.
pub const TOOLBAR_PAD_Y: f32 = 4.0;
/// Bookmark bar height (`kBookmarkBarButtonHeight = 24` + padding).
pub const BOOKMARKS_BAR_H: f32 = 28.0;

/// ⓩ The vertical rail's width (§54, ADR-0026). ZIM-local — upstream
/// ships no vertical strip constant; the founder's Discord/Edge
/// reference and the readable-row law agree on ~240 DIP: a row fits a
/// title, its glyph, and the close button without crowding.
pub const RAIL_WIDTH: f32 = 240.0;
/// ⓩ The rail rows' breathing room (ZIM-local; no upstream analogue —
/// the horizontal strip's separation lives in the 18px overlap, which
/// rows do not have).
pub const ROW_GAP: f32 = 4.0;

/// `GetTabOverlap() = 2 × bottom radius − (separator width + margins)` —
/// tab_style.cc. 2×12 − (2 + 4) = 18.
pub fn tab_overlap() -> f32 {
    2.0 * CORNER_BOTTOM - (SEPARATOR_W + 2.0 * SEPARATOR_MARGIN)
}

/// `GetStandardWidth()` — 232 + 2 × 12 = 256.
pub fn standard_width() -> f32 {
    TAB_WIDTH + 2.0 * CORNER_BOTTOM
}

/// `GetPinnedWidth()` — 24 + contents insets.
pub fn pinned_width() -> f32 {
    PINNED_CONTENT + 2.0 * CONTENT_INSET_X
}

/// `GetMinimumActiveWidth()` — close button + insets.
pub fn min_active_width() -> f32 {
    CLOSE_BUTTON + 2.0 * CONTENT_INSET_X
}

/// `GetMinimumInactiveWidth()` — interior 16 − separator + overlap.
pub fn min_inactive_width() -> f32 {
    MIN_INACTIVE_INTERIOR - SEPARATOR_W + tab_overlap()
}

/// The frame band's height above the content webviews.
pub fn header_height(bookmarks_bar_visible: bool) -> f32 {
    let strip = TAB_HEIGHT + STRIP_PADDING;
    let toolbar = LOCATION_BAR_H + 2.0 * TOOLBAR_PAD_Y;
    let bookmarks = if bookmarks_bar_visible {
        BOOKMARKS_BAR_H
    } else {
        0.0
    };
    strip + toolbar + bookmarks
}

/// One laid-out slot — what the frame layer renders. The model owns the
/// geometry (single source of truth; the frame is a view of it).
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct Slot {
    pub id: u32,
    pub x: f32,
    pub width: f32,
    /// The slot's TOP edge. The horizontal strip's tabs all share the
    /// band (y = 0 — the frame pins them to the strip's floor); the
    /// vertical rail's rows stack by it.
    pub y: f32,
    /// The slot's height: TAB_HEIGHT both ways — the rail renders
    /// full-width rows at the tab's own height (ADR-0026).
    pub height: f32,
    pub pinned: bool,
    /// A closing tab's slot shrinks to the overlap width and fades
    /// (`TransformForPinnednessAndOpenness`, IsClosed branch).
    pub closing: bool,
    /// True for a group's header chip: `id` is then the GROUP id, not a
    /// tab id (tab_group_header.h — every group leads with its chip, and
    /// a collapsed group IS the chip).
    pub header: bool,
}

/// The group header chip's width. Upstream measures the label with the
/// real font (tab_group_header.cc lays out its title); without text
/// shaping here the width approximates at ~7 DIP per label character
/// inside fixed padding — a documented divergence, porting-spec §1. The
/// padding covers the chip's 2 × 12 inset plus a breath.
fn group_header_width(label: &str) -> f32 {
    const PAD: f32 = 22.0;
    const PER_CHAR: f32 = 7.0;
    const MIN: f32 = 28.0;
    const MAX: f32 = 140.0;
    (PAD + PER_CHAR * label.chars().count() as f32).clamp(MIN, MAX)
}

/// The width law, ported from tab_width_constraints.cc: closed ⇒ overlap,
/// pinned ⇒ pinned width, otherwise min(active/inactive) .. standard.
/// Group law (tab_group_header.h / tab_group_views.cc): every group
/// leads with its header chip; a collapsed group reduces to the chip.
///
/// Distribution rule (documented divergence, porting spec §1): when the
/// strip overflows, inactive tabs shrink to their minimum first, then the
/// active tab, then everything clamps at its minimum (upstream scrolls
/// with chevron buttons — reserved, not ported in this phase).
pub fn compute_layout(
    strip_width: f32,
    tabs: &[(u32, bool, bool, Option<u32>)], // (id, pinned, closing, group)
    active: u32,
    groups: &[(u32, bool, &str)], // (id, collapsed, label)
) -> Vec<Slot> {
    if tabs.is_empty() {
        return Vec::new();
    }
    let overlap = tab_overlap();
    // The strip's usable budget: the leading inset (kTabStripPadding
    // doubles as the horizontal strip inset, as it does for the height),
    // the new-tab button, and the caption area carved off the right end.
    let budget = (strip_width - STRIP_PADDING - NEW_TAB_BUTTON_W - WINDOW_CONTROLS_W).max(0.0);

    // The group walk: which group leads where, and which tabs are
    // visible at all (a collapsed group's tabs are not laid out — the
    // chip stands for the whole group).
    let collapsed_of = |gid: u32| {
        groups
            .iter()
            .find(|g| g.0 == gid)
            .map(|g| g.1)
            .unwrap_or(false)
    };
    let label_of = |gid: u32| {
        groups
            .iter()
            .find(|g| g.0 == gid)
            .map(|g| g.2.to_owned())
            .unwrap_or_default()
    };
    let mut seen: Vec<u32> = Vec::new();
    let mut visible: Vec<bool> = Vec::new();
    let mut headers: Vec<Option<u32>> = Vec::new(); // header slot before tab i?
    for tab in tabs {
        let group = tab.3;
        let leads = match group {
            Some(gid) => {
                if seen.contains(&gid) {
                    false
                } else {
                    seen.push(gid);
                    true
                }
            }
            None => false,
        };
        headers.push(if leads { group } else { None });
        let hidden = group.map(collapsed_of).unwrap_or(false);
        visible.push(!hidden);
    }

    let laid_out: Vec<usize> = (0..tabs.len())
        .filter(|i| visible[*i] || headers[*i].is_some())
        .collect();
    if laid_out.is_empty() {
        return Vec::new();
    }

    let width_of = |i: usize| -> f32 {
        let (id, pinned, closing, _) = tabs[i];
        if closing {
            return overlap;
        }
        let floor = if id == active {
            min_active_width()
        } else {
            min_inactive_width()
        };
        if pinned {
            pinned_width().max(floor)
        } else {
            standard_width().max(floor)
        }
    };

    let preferred: f32 = laid_out
        .iter()
        .map(|i| {
            headers[*i]
                .map(|gid| group_header_width(&label_of(gid)))
                .unwrap_or(0.0)
                + if visible[*i] { width_of(*i) } else { 0.0 }
        })
        .sum::<f32>()
        - overlap * (laid_out.len().max(1) as f32 - 1.0);

    let mut widths: Vec<f32> = laid_out
        .iter()
        .map(|i| if visible[*i] { width_of(*i) } else { 0.0 })
        .collect();
    let header_w: Vec<Option<f32>> = laid_out
        .iter()
        .map(|i| headers[*i].map(|gid| group_header_width(&label_of(gid))))
        .collect();

    if preferred > budget {
        // Shrink inactive to their minimum, keep the active tab alive
        // longest (the LayoutDomain crossover, simplified).
        for (k, i) in laid_out.iter().enumerate() {
            let (id, pinned, closing, _) = tabs[*i];
            if !pinned && !closing && id != active && visible[*i] {
                widths[k] = min_inactive_width();
            }
        }
        // The laid-out units' total width (tabs + chips minus overlaps).
        let sum_units = |widths: &[f32], header_w: &[Option<f32>]| -> f32 {
            widths
                .iter()
                .zip(header_w)
                .map(|(w, h)| w + h.unwrap_or(0.0))
                .sum::<f32>()
                - overlap * (widths.len().max(1) as f32 - 1.0)
        };
        let mut total = sum_units(&widths, &header_w);
        if total > budget {
            // Even the active tab goes to its minimum.
            for (k, i) in laid_out.iter().enumerate() {
                let (id, pinned, closing, _) = tabs[*i];
                if !pinned && !closing && id == active && visible[*i] {
                    widths[k] = min_active_width();
                }
            }
            total = sum_units(&widths, &header_w);
            if total > budget {
                // Clamped overflow: upstream would scroll (reserved).
                return finish(
                    tabs, &laid_out, &headers, &visible, &widths, &header_w, overlap,
                );
            }
        }
        // Distribute the leftover to the active tab first, then evenly to
        // the inactive ones, up to standard width.
        let mut free = budget - total;
        for (k, i) in laid_out.iter().enumerate() {
            if free <= 0.0 {
                break;
            }
            let (id, pinned, closing, _) = tabs[*i];
            if !pinned && !closing && visible[*i] {
                let room = standard_width() - widths[k];
                let give = if id == active {
                    room.min(free)
                } else {
                    room.min(free * 0.5)
                };
                widths[k] += give;
                free -= give;
            }
        }
    }
    finish(
        tabs, &laid_out, &headers, &visible, &widths, &header_w, overlap,
    )
}

/// Emit the laid-out units: each unit is [header chip?] + [tab?], the
/// overlap chain runs between units (chip and its own tab are adjacent,
/// not overlapped — the chip is the group's leading edge).
#[allow(clippy::too_many_arguments)]
fn finish(
    tabs: &[(u32, bool, bool, Option<u32>)],
    laid_out: &[usize],
    headers: &[Option<u32>],
    visible: &[bool],
    widths: &[f32],
    header_w: &[Option<f32>],
    overlap: f32,
) -> Vec<Slot> {
    let mut x = STRIP_PADDING;
    let mut slots = Vec::new();
    for (k, i) in laid_out.iter().enumerate() {
        let mut unit_end = x;
        if let (Some(gid), Some(w)) = (headers[*i], header_w[k]) {
            slots.push(Slot {
                id: gid,
                x,
                width: w,
                y: 0.0,
                height: TAB_HEIGHT,
                pinned: false,
                closing: false,
                header: true,
            });
            unit_end = x + w;
        }
        if visible[*i] {
            let (id, pinned, closing, _) = tabs[*i];
            slots.push(Slot {
                id,
                x: unit_end,
                width: widths[k],
                y: 0.0,
                height: TAB_HEIGHT,
                pinned,
                closing,
                header: false,
            });
            unit_end += widths[k];
        }
        x = unit_end - overlap;
    }
    slots
}

/// Which tab index a drop at `x` lands on (drag reorder): the slot whose
/// center is past the point, per Chrome's reorder hit-testing. Header
/// chips are not drop targets — only real tabs count.
pub fn drop_index(slots: &[Slot], x: f32) -> usize {
    let tabs: Vec<&Slot> = slots.iter().filter(|s| !s.header).collect();
    let mut index = tabs.len();
    for (i, slot) in tabs.iter().enumerate() {
        if x < slot.x + slot.width * 0.5 {
            index = i;
            break;
        }
    }
    index
}

/// §54's vertical presentation (ADR-0026): the SAME tab objects as a
/// left rail — full-width rows, the pinned head compact at the top,
/// groups as stacked chips. The model, its identity rules, and the test
/// seams are untouched; only the presentation turns (the ADR's own
/// law: "exactly as deep as presentation").
///
/// Rows never overlap — the 18px chain is the horizontal strip's
/// corner-curve law, and rows have no curves to tuck. The width law
/// collapses for the same reason: every row spans the rail's interior
/// (`rail_width − 2×STRIP_PADDING`); the shrink law is horizontal-only
/// (a rail scrolls, it does not squeeze).
pub fn compute_layout_vertical(
    rail_width: f32,
    tabs: &[(u32, bool, bool, Option<u32>)], // (id, pinned, closing, group)
    groups: &[(u32, bool, &str)],            // (id, collapsed, label)
) -> Vec<Slot> {
    if tabs.is_empty() {
        return Vec::new();
    }
    // The group walk, straight from the horizontal law: which group
    // leads where, and which tabs are visible at all (a collapsed
    // group's tabs are not laid out — the chip stands for the group).
    let collapsed_of = |gid: u32| {
        groups
            .iter()
            .find(|g| g.0 == gid)
            .map(|g| g.1)
            .unwrap_or(false)
    };
    let mut seen: Vec<u32> = Vec::new();
    let mut y = STRIP_PADDING;
    let row_w = (rail_width - 2.0 * STRIP_PADDING).max(0.0);
    let mut slots = Vec::new();
    for tab in tabs {
        let group = tab.3;
        let leads = match group {
            Some(gid) => {
                if seen.contains(&gid) {
                    false
                } else {
                    seen.push(gid);
                    true
                }
            }
            None => false,
        };
        if let Some(gid) = leads.then_some(group).flatten() {
            slots.push(Slot {
                id: gid,
                x: STRIP_PADDING,
                width: row_w,
                y,
                height: TAB_HEIGHT,
                pinned: false,
                closing: false,
                header: true,
            });
            y += TAB_HEIGHT + ROW_GAP;
        }
        if group.map(collapsed_of).unwrap_or(false) {
            continue;
        }
        let (id, pinned, closing, _) = *tab;
        slots.push(Slot {
            id,
            x: STRIP_PADDING,
            width: row_w,
            y,
            height: TAB_HEIGHT,
            pinned,
            closing,
            header: false,
        });
        y += TAB_HEIGHT + ROW_GAP;
    }
    slots
}

/// The vertical twin of [`drop_index`]: which tab index a drop at `y`
/// lands on — the row whose center is past the point. Header chips are
/// not drop targets; only real tabs count.
pub fn drop_index_vertical(slots: &[Slot], y: f32) -> usize {
    let tabs: Vec<&Slot> = slots.iter().filter(|s| !s.header).collect();
    let mut index = tabs.len();
    for (i, slot) in tabs.iter().enumerate() {
        if y < slot.y + slot.height * 0.5 {
            index = i;
            break;
        }
    }
    index
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(tabs: &[(u32, bool, bool)]) -> Vec<(u32, bool, bool, Option<u32>)> {
        tabs.iter().map(|(id, p, c)| (*id, *p, *c, None)).collect()
    }

    #[test]
    fn constants_match_the_source() {
        assert_eq!(tab_overlap(), 18.0);
        assert_eq!(standard_width(), 256.0);
        assert_eq!(pinned_width(), 40.0);
        assert_eq!(min_active_width(), 32.0);
        assert_eq!(min_inactive_width(), 32.0);
    }

    #[test]
    fn preferred_tabs_get_standard_width() {
        let tabs = plain(&[(1u32, false, false), (2, false, false), (3, false, false)]);
        let slots = compute_layout(1200.0, &tabs, 1, &[]);
        assert!(slots.iter().all(|s| (s.width - 256.0).abs() < 0.01));
        assert_eq!(slots[0].x, STRIP_PADDING);
        assert_eq!(slots[1].x, STRIP_PADDING + 256.0 - 18.0);
    }

    #[test]
    fn the_strip_never_slides_under_the_caption_area() {
        // Enough tabs to overflow: every slot must end before the window
        // controls' left edge (the strip_width minus the controls). That
        // is the whole point of the budget's caption reservation.
        let tabs: Vec<(u32, bool, bool, Option<u32>)> =
            (1..=30).map(|i| (i, false, false, None)).collect();
        let slots = compute_layout(900.0, &tabs, 1, &[]);
        let controls_left = 900.0 - WINDOW_CONTROLS_W;
        for slot in &slots {
            assert!(
                slot.x + slot.width <= controls_left + 0.01,
                "slot {} ends at {} past the controls at {}",
                slot.id,
                slot.x + slot.width,
                controls_left,
            );
        }
    }

    #[test]
    fn pinned_tabs_are_fixed_width() {
        let tabs = plain(&[(1u32, true, false), (2, false, false)]);
        let slots = compute_layout(1200.0, &tabs, 2, &[]);
        assert_eq!(slots[0].width, 40.0);
        assert_eq!(slots[1].width, 256.0);
    }

    #[test]
    fn overflow_shrinks_inactive_first() {
        let tabs: Vec<(u32, bool, bool, Option<u32>)> =
            (1..=20).map(|i| (i, false, false, None)).collect();
        let slots = compute_layout(600.0, &tabs, 1, &[]);
        let active = slots.iter().find(|s| s.id == 1).unwrap();
        let inactive = slots.iter().find(|s| s.id == 10).unwrap();
        assert!(active.width >= inactive.width);
        assert!(inactive.width >= min_inactive_width() - 0.01);
    }

    #[test]
    fn closing_tab_collapses_to_overlap() {
        let tabs = plain(&[(1u32, false, false), (2, false, true)]);
        let slots = compute_layout(1200.0, &tabs, 1, &[]);
        assert_eq!(slots[1].width, tab_overlap());
        assert!(slots[1].closing);
    }

    #[test]
    fn drop_index_follows_centers() {
        let tabs = plain(&[(1u32, false, false), (2, false, false), (3, false, false)]);
        let slots = compute_layout(1200.0, &tabs, 1, &[]);
        assert_eq!(drop_index(&slots, 10.0), 0);
        assert_eq!(drop_index(&slots, 240.0), 1);
        assert_eq!(drop_index(&slots, 5000.0), 3);
    }

    #[test]
    fn every_group_leads_with_its_header_chip() {
        // Tabs 2 and 3 share group 7: a chip precedes tab 2, tab 3
        // follows directly (no second chip mid-group).
        let tabs = vec![
            (1u32, false, false, None),
            (2, false, false, Some(7)),
            (3, false, false, Some(7)),
        ];
        let slots = compute_layout(1200.0, &tabs, 1, &[(7, false, "survival")]);
        let chip = slots.iter().find(|s| s.header).expect("the chip");
        assert_eq!(chip.id, 7);
        assert_eq!(chip.width, group_header_width("survival"));
        assert!(
            chip.x < slots.iter().find(|s| s.id == 2).unwrap().x,
            "the chip leads its group",
        );
        assert_eq!(slots.iter().filter(|s| s.header).count(), 1);
    }

    #[test]
    fn a_collapsed_group_is_only_its_chip() {
        let tabs = vec![
            (1u32, false, false, None),
            (2, false, false, Some(7)),
            (3, false, false, Some(7)),
        ];
        let slots = compute_layout(1200.0, &tabs, 1, &[(7, true, "survival")]);
        assert_eq!(slots.iter().filter(|s| s.header).count(), 1);
        assert!(
            slots.iter().all(|s| s.header || s.id == 1),
            "group tabs hide"
        );
    }

    #[test]
    fn drop_index_skips_header_chips() {
        let tabs = vec![(1u32, false, false, Some(7)), (2, false, false, Some(7))];
        let slots = compute_layout(1200.0, &tabs, 1, &[(7, false, "g")]);
        // A drop on the chip itself still lands by the tab centers.
        let first_tab = slots.iter().find(|s| !s.header).unwrap();
        let mid_chip = slots.iter().find(|s| s.header).unwrap().x + 2.0;
        assert_eq!(drop_index(&slots, mid_chip), 0);
        let mid_first = first_tab.x + first_tab.width * 0.5 + 1.0;
        assert_eq!(drop_index(&slots, mid_first), 1);
    }

    // -- The vertical rail (§54, ADR-0026) ----------------------------------

    #[test]
    fn vertical_rows_stack_from_the_padding_with_gaps() {
        let tabs = plain(&[(1u32, false, false), (2, false, false), (3, false, false)]);
        let slots = compute_layout_vertical(RAIL_WIDTH, &tabs, &[]);
        assert_eq!(slots.len(), 3);
        for (i, slot) in slots.iter().enumerate() {
            let want_y = STRIP_PADDING + i as f32 * (TAB_HEIGHT + ROW_GAP);
            assert!((slot.y - want_y).abs() < 0.01, "row {i} at y {}", slot.y);
            assert_eq!(slot.height, TAB_HEIGHT);
            // Every row spans the rail's interior — the width law is
            // horizontal-only; a rail scrolls, it does not squeeze.
            assert_eq!(slot.x, STRIP_PADDING);
            assert_eq!(slot.width, RAIL_WIDTH - 2.0 * STRIP_PADDING);
        }
    }

    #[test]
    fn vertical_drop_follows_row_centers() {
        let tabs = plain(&[(1u32, false, false), (2, false, false), (3, false, false)]);
        let slots = compute_layout_vertical(RAIL_WIDTH, &tabs, &[]);
        assert_eq!(drop_index_vertical(&slots, 10.0), 0);
        let second_center = slots[1].y + slots[1].height * 0.5 + 1.0;
        assert_eq!(drop_index_vertical(&slots, second_center), 2);
        assert_eq!(drop_index_vertical(&slots, 100_000.0), 3);
    }

    #[test]
    fn vertical_group_leads_with_a_chip_row_and_collapses_to_it() {
        let tabs = vec![
            (1u32, false, false, None),
            (2, false, false, Some(7)),
            (3, false, false, Some(7)),
        ];
        let open = compute_layout_vertical(RAIL_WIDTH, &tabs, &[(7, false, "survival")]);
        let chip = open.iter().find(|s| s.header).expect("the chip row");
        assert_eq!(chip.id, 7);
        let tab2 = open.iter().find(|s| s.id == 2).unwrap();
        assert!(
            chip.y < tab2.y,
            "the chip row leads its group in the rail too"
        );
        // The collapsed rail: the chip stands for the whole group.
        let collapsed = compute_layout_vertical(RAIL_WIDTH, &tabs, &[(7, true, "survival")]);
        assert_eq!(collapsed.iter().filter(|s| s.header).count(), 1);
        assert!(collapsed.iter().all(|s| s.header || s.id == 1));
    }

    #[test]
    fn vertical_horizontal_slots_carry_their_own_axis() {
        // The horizontal strip's tabs all share the band: y = 0, height
        // = TAB_HEIGHT — the rail's fields exist in both laws so the
        // frame renders either from the same snapshot type.
        let tabs = plain(&[(1u32, false, false)]);
        let slots = compute_layout(1200.0, &tabs, 1, &[]);
        assert_eq!(slots[0].y, 0.0);
        assert_eq!(slots[0].height, TAB_HEIGHT);
    }
}
