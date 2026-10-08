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
/// `GetTopCornerRadius() = 10` — tab_style.cc.
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
/// `kSeparatorHeight = 20` (touch_ui false) — layout_constants.cc.
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
/// `kLocationBarHeight = 34` — layout_constants.cc.
pub const LOCATION_BAR_H: f32 = 34.0;
/// Toolbar vertical padding around the location bar.
pub const TOOLBAR_PAD_Y: f32 = 4.0;
/// Bookmark bar height (`kBookmarkBarButtonHeight = 24` + padding).
pub const BOOKMARKS_BAR_H: f32 = 28.0;

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
    let bookmarks = if bookmarks_bar_visible { BOOKMARKS_BAR_H } else { 0.0 };
    strip + toolbar + bookmarks
}

/// One laid-out slot — what the frame layer renders. The model owns the
/// geometry (single source of truth; the frame is a view of it).
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct Slot {
    pub id: u32,
    pub x: f32,
    pub width: f32,
    pub pinned: bool,
    /// A closing tab's slot shrinks to the overlap width and fades
    /// (`TransformForPinnednessAndOpenness`, IsClosed branch).
    pub closing: bool,
}

/// The width law, ported from tab_width_constraints.cc: closed ⇒ overlap,
/// pinned ⇒ pinned width, otherwise min(active/inactive) .. standard.
///
/// Distribution rule (documented divergence, porting spec §1): when the
/// strip overflows, inactive tabs shrink to their minimum first, then the
/// active tab, then everything clamps at its minimum (upstream scrolls
/// with chevron buttons — reserved, not ported in this phase).
pub fn compute_layout(
    strip_width: f32,
    tabs: &[(u32, bool, bool)], // (id, pinned, closing)
    active: u32,
) -> Vec<Slot> {
    if tabs.is_empty() {
        return Vec::new();
    }
    let overlap = tab_overlap();
    let budget = (strip_width - NEW_TAB_BUTTON_W).max(0.0);
    let unpinned: Vec<&(u32, bool, bool)> = tabs.iter().filter(|t| !t.1 && !t.2).collect();
    let pinned_count = tabs.iter().filter(|t| t.1 && !t.2).count();
    let pinned_total = pinned_count as f32 * pinned_width();

    let preferred = unpinned.len() as f32 * standard_width()
        + pinned_total
        - overlap * (tabs.len().max(1) as f32 - 1.0);
    let mut widths: Vec<f32> = tabs
        .iter()
        .map(|(_, pinned, closing)| {
            if *closing {
                overlap
            } else if *pinned {
                pinned_width()
            } else {
                standard_width()
            }
        })
        .collect();

    if preferred > budget && !unpinned.is_empty() {
        // Shrink inactive to their minimum, keep the active tab alive
        // longest (the LayoutDomain crossover, simplified).
        for (i, (id, pinned, closing)) in tabs.iter().enumerate() {
            if !*pinned && !*closing && *id != active {
                widths[i] = min_inactive_width();
            }
        }
        let total: f32 =
            widths.iter().sum::<f32>() - overlap * (tabs.len().max(1) as f32 - 1.0);
        if total > budget {
            // Even the active tab goes to its minimum.
            for (i, (id, pinned, closing)) in tabs.iter().enumerate() {
                if !*pinned && !*closing && *id == active {
                    widths[i] = min_active_width();
                }
            }
            let total: f32 =
                widths.iter().sum::<f32>() - overlap * (tabs.len().max(1) as f32 - 1.0);
            if total > budget {
                // Clamped overflow: upstream would scroll (reserved).
                return finish(tabs, widths, overlap);
            }
        }
        // Distribute the leftover to the active tab first, then evenly to
        // the inactive ones, up to standard width.
        let mut free = budget
            - (widths.iter().sum::<f32>() - overlap * (tabs.len().max(1) as f32 - 1.0));
        for (i, (id, pinned, closing)) in tabs.iter().enumerate() {
            if free <= 0.0 {
                break;
            }
            if !*pinned && !*closing {
                let room = standard_width() - widths[i];
                let give = if *id == active { room.min(free) } else { room.min(free * 0.5) };
                widths[i] += give;
                free -= give;
            }
        }
    }
    finish(tabs, widths, overlap)
}

fn finish(tabs: &[(u32, bool, bool)], widths: Vec<f32>, overlap: f32) -> Vec<Slot> {
    let mut x = 0.0;
    tabs.iter()
        .zip(widths)
        .map(|((id, pinned, closing), w)| {
            let slot = Slot { id: *id, x, width: w, pinned: *pinned, closing: *closing };
            x += w - overlap;
            slot
        })
        .collect()
}

/// Which tab index a drop at `x` lands on (drag reorder): the slot whose
/// center is past the point, per Chrome's reorder hit-testing.
pub fn drop_index(slots: &[Slot], x: f32) -> usize {
    let mut index = slots.len();
    for (i, slot) in slots.iter().enumerate() {
        if x < slot.x + slot.width * 0.5 {
            index = i;
            break;
        }
    }
    index
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let tabs = vec![(1u32, false, false), (2, false, false), (3, false, false)];
        let slots = compute_layout(1200.0, &tabs, 1);
        assert!(slots.iter().all(|s| (s.width - 256.0).abs() < 0.01));
        assert_eq!(slots[1].x, 256.0 - 18.0);
    }

    #[test]
    fn pinned_tabs_are_fixed_width() {
        let tabs = vec![(1u32, true, false), (2, false, false)];
        let slots = compute_layout(1200.0, &tabs, 2);
        assert_eq!(slots[0].width, 40.0);
        assert_eq!(slots[1].width, 256.0);
    }

    #[test]
    fn overflow_shrinks_inactive_first() {
        let tabs: Vec<(u32, bool, bool)> = (1..=20).map(|i| (i, false, false)).collect();
        let slots = compute_layout(600.0, &tabs, 1);
        let active = slots.iter().find(|s| s.id == 1).unwrap();
        let inactive = slots.iter().find(|s| s.id == 10).unwrap();
        assert!(active.width >= inactive.width);
        assert!(inactive.width >= min_inactive_width() - 0.01);
    }

    #[test]
    fn closing_tab_collapses_to_overlap() {
        let tabs = vec![(1u32, false, false), (2, false, true)];
        let slots = compute_layout(1200.0, &tabs, 1);
        assert_eq!(slots[1].width, tab_overlap());
        assert!(slots[1].closing);
    }

    #[test]
    fn drop_index_follows_centers() {
        let tabs = vec![(1u32, false, false), (2, false, false), (3, false, false)];
        let slots = compute_layout(1200.0, &tabs, 1);
        assert_eq!(drop_index(&slots, 10.0), 0);
        assert_eq!(drop_index(&slots, 240.0), 1);
        assert_eq!(drop_index(&slots, 5000.0), 3);
    }
}
