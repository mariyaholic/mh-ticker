//! Where panes go.
//!
//! Every function here is pure: no `HWND`, no `HDC`, no config, no clock. `solve` is the whole idea —
//! a composition in, one rectangle per pane out — and it is testable without a window, which is why
//! it is a module rather than another three hundred lines in `ui.rs`.
//!
//! **P0 status.** The app calls `grid` and nothing else: today's panel is a one-pane composition, and
//! today's window hugs its content instead of being given an area. `solve` is exercised by the tests
//! below until P1 hands it the client rect (WINDOW.md §9).

// P0 ships the pane model a phase before the window that consumes it: `solve`, the rect helpers and
// the panes that are not the watchlist are used by the tests below and by nothing else until P1.
// Drop this allow the moment `solve` has a caller.
#![allow(dead_code)]

/// The panel's metrics, in device-independent pixels.
///
/// 150 tall and 236 wide are not round numbers on purpose: the day-range bar, the 52-week line and
/// its date need the extra strip, and squeezing them into 132 overlapped the last hour's line.
pub const PAD: i32 = 16;
pub const GAP: i32 = 10;
pub const CARD_W: i32 = 236;
pub const CARD_H: i32 = 150;
pub const HEADER: i32 = 48;

/// The widest the card grid gets before it adds a row instead of a column. `grid` (content-sized)
/// stops here; `grid_in` (window-sized) goes up to [`MAX_COLS`] when the window has the width.
pub const COLS: usize = 3;
/// Six columns is 1,514 DIP wide: a 1080p desktop at 100 %, or any 1440p. Wider than that and the
/// cards stop being scannable, so the window just gets gutters (WINDOW.md §4).
pub const MAX_COLS: usize = 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Size {
    pub w: i32,
    pub h: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub fn size(&self) -> Size {
        Size { w: self.w, h: self.h }
    }
}

/// The card grid for `n` cards: its size, and how many columns it settled on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Grid {
    pub w: i32,
    pub h: i32,
    pub cols: usize,
}

impl Grid {
    pub fn size(&self) -> Size {
        Size { w: self.w, h: self.h }
    }
}

/// A pane is a thing that can be given a rectangle. Only `Watchlist` draws today.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pane {
    Watchlist,
    Chart,
    Rail,
    Timeline,
    Wall { cols: usize, rows: usize },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Split {
    Row,
    Column,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    pub pane: Pane,
    /// Share of the space left over once every minimum is met — not a share of the total.
    pub weight: f32,
    /// Below this the pane cannot draw itself honestly, so the solver never goes under it.
    pub min: Size,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Compo {
    pub split: Split,
    pub gap: i32,
    pub nodes: Vec<Node>,
}

fn s(v: i32, dpi: i32) -> i32 {
    v * dpi / 96
}

/// The card grid for `n` cards at `dpi`.
pub fn grid(n: usize, dpi: i32) -> Grid {
    let cols = n.clamp(1, COLS);
    let rows = n.div_ceil(COLS).max(1) as i32;
    Grid {
        w: 2 * s(PAD, dpi) + cols as i32 * s(CARD_W, dpi) + (cols as i32 - 1) * s(GAP, dpi),
        h: s(HEADER, dpi) + rows * s(CARD_H, dpi) + (rows - 1) * s(GAP, dpi),
        cols,
    }
}

/// How many cards fit across `w`, at the fixed card size. Never fewer than one.
pub fn cols_for(w: i32, dpi: i32) -> usize {
    let usable = w - 2 * s(PAD, dpi) + s(GAP, dpi);
    let step = (s(CARD_W, dpi) + s(GAP, dpi)).max(1);
    (usable.max(0) / step).clamp(1, MAX_COLS as i32) as usize
}

/// The card grid for `n` cards in a window `w` wide: columns come from the width, rows from the
/// count. `w` is kept as given — the caller asked for a window this wide, and gutters are the honest
/// answer when the cards do not divide it evenly.
pub fn grid_in(w: i32, n: usize, dpi: i32) -> Grid {
    let cols = cols_for(w, dpi);
    let rows = n.div_ceil(cols).max(1) as i32;
    Grid {
        w,
        h: s(HEADER, dpi) + rows * s(CARD_H, dpi) + (rows - 1) * s(GAP, dpi),
        cols,
    }
}

impl Compo {
    /// Today's panel: one watchlist pane, sized to its content.
    pub fn panel(n: usize, dpi: i32) -> Self {
        let g = grid(n, dpi);
        Self { split: Split::Row, gap: 0, nodes: vec![Node { pane: Pane::Watchlist, weight: 1.0, min: g.size() }] }
    }

    pub fn row(nodes: Vec<Node>, gap: i32) -> Self {
        Self { split: Split::Row, gap, nodes }
    }

    pub fn column(nodes: Vec<Node>, gap: i32) -> Self {
        Self { split: Split::Column, gap, nodes }
    }

    /// The size at which every pane sits at its minimum, plus the gaps between them.
    pub fn natural(&self, dpi: i32) -> Size {
        let gap = s(self.gap, dpi);
        let n = self.nodes.len() as i32;
        let span = self.nodes.iter().map(|nd| axis_min(nd, self.split)).sum::<i32>() + gap * (n - 1).max(0);
        match self.split {
            Split::Row => Size { w: span, h: self.nodes.iter().map(|nd| nd.min.h).max().unwrap_or(0) },
            Split::Column => Size { w: self.nodes.iter().map(|nd| nd.min.w).max().unwrap_or(0), h: span },
        }
    }
}

fn axis_min(nd: &Node, split: Split) -> i32 {
    match split {
        Split::Row => nd.min.w,
        Split::Column => nd.min.h,
    }
}

/// Place every pane in `area`.
///
/// Each pane gets its minimum and the leftover goes by weight, so a pane never drops below a size it
/// can draw at. A window too small to hold the composition overflows at the minimums rather than
/// producing negative rectangles: the caller clamps the window, it does not get to shrink a pane into
/// illegibility.
pub fn solve(c: &Compo, area: Rect, dpi: i32) -> Vec<(Pane, Rect)> {
    let gap = s(c.gap, dpi);
    let n = c.nodes.len();
    if n == 0 {
        return Vec::new();
    }
    let avail = match c.split {
        Split::Row => area.w - gap * (n as i32 - 1),
        Split::Column => area.h - gap * (n as i32 - 1),
    };
    let mins: Vec<i32> = c.nodes.iter().map(|nd| axis_min(nd, c.split)).collect();
    let total_min: i32 = mins.iter().sum();
    let free = (avail - total_min).max(0);
    let weight_sum: f32 = c.nodes.iter().map(|nd| nd.weight.max(0.0)).sum();

    // Floor the shares, then hand the rounding remainder to the heaviest panes first, so the sizes
    // always add up to exactly `avail` — a splitter that loses a pixel per frame looks like drift.
    let mut sizes: Vec<i32> = mins.clone();
    if weight_sum > 0.0 && free > 0 {
        let mut order: Vec<usize> = (0..n).collect();
        order.sort_by(|&a, &b| c.nodes[b].weight.total_cmp(&c.nodes[a].weight));
        let mut given = 0;
        for &i in &order {
            let share = (free as f32 * c.nodes[i].weight.max(0.0) / weight_sum).floor() as i32;
            sizes[i] += share;
            given += share;
        }
        let rem = (free - given).max(0);
        for &i in order.iter().cycle().take(rem as usize) {
            sizes[i] += 1;
        }
    }

    let mut out = Vec::with_capacity(n);
    let mut at = match c.split {
        Split::Row => area.x,
        Split::Column => area.y,
    };
    for (i, nd) in c.nodes.iter().enumerate() {
        let rect = match c.split {
            Split::Row => Rect { x: at, y: area.y, w: sizes[i], h: area.h },
            Split::Column => Rect { x: area.x, y: at, w: area.w, h: sizes[i] },
        };
        out.push((nd.pane, rect));
        at += sizes[i] + gap;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(p: Pane, weight: f32, w: i32, h: i32) -> Node {
        Node { pane: p, weight, min: Size { w, h } }
    }

    #[test]
    fn the_shipped_panel_geometries_are_unchanged() {
        // The numbers 1.4.0 computed by hand in `geometry()`; shots/*.png is the pixel proof, this is
        // the arithmetic one.
        assert_eq!(grid(1, 96), Grid { w: 268, h: 198, cols: 1 });
        assert_eq!(grid(3, 96), Grid { w: 760, h: 198, cols: 3 });
        assert_eq!(grid(6, 96), Grid { w: 760, h: 358, cols: 3 });
        assert_eq!(grid(12, 96), Grid { w: 760, h: 678, cols: 3 });
    }

    #[test]
    fn a_single_pane_gets_the_whole_area() {
        let c = Compo::panel(6, 96);
        let area = Rect { x: 0, y: 0, w: 760, h: 358 };
        assert_eq!(solve(&c, area, 96), vec![(Pane::Watchlist, area)]);
    }

    #[test]
    fn leftover_goes_by_weight_and_minimums_survive() {
        let c = Compo::row(vec![node(Pane::Watchlist, 1.0, 400, 300), node(Pane::Chart, 2.0, 380, 260)], 10);
        let solved = solve(&c, Rect { x: 0, y: 0, w: 980, h: 300 }, 96);
        // 980 - 10 gap - 780 mins = 190 free, split 1:2 -> 63 / 127.
        assert_eq!(solved[0].1, Rect { x: 0, y: 0, w: 463, h: 300 });
        assert_eq!(solved[1].1, Rect { x: 473, y: 0, w: 507, h: 300 });
    }

    #[test]
    fn a_window_too_small_overflows_instead_of_shrinking_past_the_minimum() {
        let c = Compo::row(vec![node(Pane::Watchlist, 1.0, 400, 300), node(Pane::Chart, 1.0, 380, 260)], 10);
        let solved = solve(&c, Rect { x: 0, y: 0, w: 500, h: 300 }, 96);
        assert_eq!((solved[0].1.w, solved[1].1.w), (400, 380));
    }

    #[test]
    fn a_column_split_uses_height_and_a_row_split_uses_width() {
        let c = Compo::column(vec![node(Pane::Chart, 1.0, 300, 200), node(Pane::Timeline, 1.0, 300, 72)], 8);
        let solved = solve(&c, Rect { x: 0, y: 40, w: 300, h: 400 }, 96);
        // 400 - 8 gap - 272 mins = 120 free, split evenly: +60 each.
        assert_eq!(solved[0].1, Rect { x: 0, y: 40, w: 300, h: 260 });
        assert_eq!(solved[1].1, Rect { x: 0, y: 308, w: 300, h: 132 });
    }

    #[test]
    fn sizes_always_add_up_to_the_space_available() {
        let c = Compo::row(
            vec![
                node(Pane::Watchlist, 1.0, 100, 100),
                node(Pane::Chart, 3.0, 100, 100),
                node(Pane::Rail, 1.0, 100, 100),
            ],
            7,
        );
        // Natural width is 3 x 100 + 2 x 7 = 314; below that the panes overflow at their minimums,
        // which is the documented behaviour and is asserted separately.
        for w in [314, 315, 640, 999, 1000, 1601] {
            let solved = solve(&c, Rect { x: 0, y: 0, w, h: 200 }, 96);
            let sum: i32 = solved.iter().map(|(_, r)| r.w).sum();
            assert_eq!(sum + 7 * 2, w, "width {w} did not account for both gaps");
        }
        let tight = solve(&c, Rect { x: 0, y: 0, w: 300, h: 200 }, 96);
        assert!(tight.iter().all(|(_, r)| r.w == 100), "a tight window keeps every minimum");
    }

    #[test]
    fn six_cards_reflow_between_one_and_six_columns() {
        // One column: 268 wide. Three: 760 - the shipped panel. Six: 1,514.
        assert_eq!(cols_for(268, 96), 1);
        assert_eq!(cols_for(760, 96), 3);
        assert_eq!(cols_for(1_514, 96), 6);
        // Between breakpoints the count floors, so cards never clip at the edge.
        assert_eq!(cols_for(1_000, 96), 3);
        assert_eq!(cols_for(1_200, 96), 4);
        // And it never runs away on a very wide monitor.
        assert_eq!(cols_for(3_840, 96), MAX_COLS);
    }

    #[test]
    fn a_window_sized_grid_fills_the_width_it_was_given() {
        let g = grid_in(1_200, 12, 96);
        assert_eq!((g.w, g.cols), (1_200, 4));
        assert_eq!(g.h, 48 + 3 * 150 + 2 * 10, "12 cards at 4 across is three rows");
        // The same width at 192 dpi fits half the columns, because the cards double.
        assert_eq!(cols_for(1_200, 192), 2);
        assert_eq!(grid_in(1_200, 12, 192).cols, 2);
    }

    #[test]
    fn a_content_sized_grid_still_uses_three_columns() {
        // `grid` must not drift with `grid_in`: the shipped panel is 760 wide for any n >= 3.
        for n in [3, 4, 6, 12] {
            assert_eq!(grid(n, 96).w, 760, "n = {n}");
            assert_eq!(grid(n, 96).cols, 3);
        }
    }

    #[test]
    fn dpi_scales_the_grid_and_not_the_layout_rules() {
        let at96 = grid(6, 96);
        let at192 = grid(6, 192);
        assert_eq!((at192.w, at192.h), (at96.w * 2, at96.h * 2));
        assert_eq!(at192.cols, at96.cols);
    }

    #[test]
    fn the_natural_size_is_the_sum_of_the_minimums() {
        let c = Compo::row(vec![node(Pane::Watchlist, 1.0, 400, 300), node(Pane::Chart, 1.0, 380, 260)], 10);
        assert_eq!(c.natural(96), Size { w: 790, h: 300 });
    }
}
