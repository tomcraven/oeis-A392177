//! Monotonic cell visit order: each `u32` index maps to one board cell `(x, y)`.
//!
//! [`VisitOrder`] is selected in the sidebar and drives simulation, rendering, and viewport
//! targeting. The default [`VisitOrder::SquareSpiral`] uses the same fast path as [`crate::spiral`].

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::spiral::{self, RingOffset};

/// How the simulation scans the board when placing pieces.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VisitOrder {
    /// Counterclockwise square spiral from the origin (OEIS A316667 geometry).
    #[default]
    SquareSpiral,
    /// Same rings as the CCW spiral, but cells on each ring are visited in the opposite direction.
    SquareSpiralClockwise,
    /// Chebyshev rings (`max(|x|, |y|)`); within each ring, sort cells by `(y, x)`.
    SquareRingsScanline,
    /// Manhattan rings (`|x| + |y|`); within each ring, sort by `x` then `|y|`.
    DiamondRings,
}

impl VisitOrder {
    pub const ALL: [VisitOrder; 4] = [
        VisitOrder::SquareSpiral,
        VisitOrder::SquareSpiralClockwise,
        VisitOrder::SquareRingsScanline,
        VisitOrder::DiamondRings,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::SquareSpiral => "Square spiral (CCW)",
            Self::SquareSpiralClockwise => "Square spiral (CW)",
            Self::SquareRingsScanline => "Square rings (scanline)",
            Self::DiamondRings => "Diamond rings (Manhattan)",
        }
    }

    fn slot(self) -> usize {
        Self::ALL
            .iter()
            .position(|&o| o == self)
            .unwrap_or(0)
    }

    pub fn prev(self) -> Self {
        let n = Self::ALL.len();
        Self::ALL[(self.slot() + n - 1) % n]
    }

    pub fn next(self) -> Self {
        let n = Self::ALL.len();
        Self::ALL[(self.slot() + 1) % n]
    }

    #[inline(always)]
    pub fn index_to_xy(self, index: u32) -> (i32, i32) {
        match self {
            Self::SquareSpiral => crate::spiral::index_to_xy(index),
            Self::SquareSpiralClockwise => index_to_xy_clockwise(index),
            Self::SquareRingsScanline => index_to_xy_square_rings_scanline(index),
            Self::DiamondRings => index_to_xy_diamond_rings(index),
        }
    }

    #[inline(always)]
    pub fn xy_to_index(self, x: i32, y: i32) -> u32 {
        match self {
            Self::SquareSpiral => crate::spiral::xy_to_index(x, y),
            Self::SquareSpiralClockwise => xy_to_index_clockwise(x, y),
            Self::SquareRingsScanline => xy_to_index_square_rings_scanline(x, y),
            Self::DiamondRings => xy_to_index_diamond_rings(x, y),
        }
    }

    #[inline(always)]
    pub fn scan_step_xy(self, index: u32, xy: (i32, i32)) -> (i32, i32) {
        match self {
            Self::SquareSpiral => spiral::spiral_step(xy),
            Self::SquareSpiralClockwise => cw_scan_step_from_index(index),
            Self::SquareRingsScanline => {
                index_to_xy_square_rings_scanline(index.wrapping_add(1))
            }
            Self::DiamondRings => diamond_scan_step(xy),
        }
    }
}

#[inline(always)]
fn cw_scan_step_from_index(index: u32) -> (i32, i32) {
    index_to_xy_clockwise(index.wrapping_add(1))
}

impl fmt::Display for VisitOrder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// How spiral indices map to the infinite square grid (type-level experiments).
pub trait IndexOrder: Copy + Default + Send + Sync + 'static {
    const NAME: &'static str;
    fn index_to_xy(index: u32) -> (i32, i32);
    fn xy_to_index(x: i32, y: i32) -> u32;
    fn scan_step_xy(index: u32, _xy: (i32, i32)) -> (i32, i32) {
        Self::index_to_xy(index.wrapping_add(1))
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SquareSpiral;

impl IndexOrder for SquareSpiral {
    const NAME: &'static str = "square_spiral";

    fn index_to_xy(index: u32) -> (i32, i32) {
        VisitOrder::SquareSpiral.index_to_xy(index)
    }

    fn xy_to_index(x: i32, y: i32) -> u32 {
        VisitOrder::SquareSpiral.xy_to_index(x, y)
    }

    fn scan_step_xy(_index: u32, xy: (i32, i32)) -> (i32, i32) {
        crate::spiral::spiral_step(xy)
    }
}

pub type DefaultIndexOrder = SquareSpiral;

#[inline(always)]
fn chebyshev_ring_start(ring: u32) -> u32 {
    if ring == 0 {
        0
    } else {
        (2 * ring - 1).pow(2)
    }
}

#[inline(always)]
fn index_to_xy_clockwise(index: u32) -> (i32, i32) {
    if index == 0 {
        return (0, 0);
    }
    let ro = spiral::index_to_ring_offset(index);
    if ro.ring == 0 {
        return (0, 0);
    }
    let ccw_offset = (8 * ro.ring - ro.offset) % (8 * ro.ring);
    spiral::index_to_xy_ring_offset(ro.ring, ccw_offset)
}

#[inline(always)]
fn xy_to_index_clockwise(x: i32, y: i32) -> u32 {
    if x == 0 && y == 0 {
        return 0;
    }
    let ro = spiral::xy_to_ring_offset(x, y);
    let cw_offset = (8 * ro.ring - ro.offset) % (8 * ro.ring);
    spiral::ring_offset_to_index(RingOffset {
        ring: ro.ring,
        offset: cw_offset,
    })
}

#[inline(always)]
fn chebyshev_ring_for_index(index: u32) -> u32 {
    if index == 0 {
        0
    } else {
        (index.isqrt() + 1) / 2
    }
}

/// Scanline order on Chebyshev ring `r`: bottom row, vertical sides, top row.
#[inline(always)]
fn index_to_xy_square_rings_scanline(index: u32) -> (i32, i32) {
    if index == 0 {
        return (0, 0);
    }
    let r = chebyshev_ring_for_index(index);
    let ri = r as i32;
    let start = chebyshev_ring_start(r);
    let o = index - start;
    let top = 2 * r + 1;
    if o < top {
        return (-ri + o as i32, -ri);
    }
    let mid_end = 6 * r - 1;
    if o < mid_end {
        let local = o - top;
        let row = local / 2;
        let y = -ri + 1 + row as i32;
        let x = if local & 1 == 0 { -ri } else { ri };
        return (x, y);
    }
    let x = -ri + (o - mid_end) as i32;
    (x, ri)
}

#[inline(always)]
fn xy_to_index_square_rings_scanline(x: i32, y: i32) -> u32 {
    let r = x.abs().max(y.abs()) as u32;
    let ri = r as i32;
    let start = chebyshev_ring_start(r);
    if r == 0 {
        return 0;
    }
    let top = 2 * r + 1;
    let mid_end = 6 * r - 1;
    let offset = if y == -ri {
        (x + ri) as u32
    } else if y == ri {
        mid_end + (x + ri) as u32
    } else {
        let row = (y + ri - 1) as u32;
        top + 2 * row + u32::from(x == ri)
    };
    start + offset
}

#[inline(always)]
fn diamond_ring_start(ring: u32) -> u32 {
    if ring == 0 {
        0
    } else {
        1 + 2 * ring * (ring - 1)
    }
}

#[inline(always)]
fn diamond_ring_for_index(index: u32) -> u32 {
    if index == 0 {
        return 0;
    }
    let u = (index - 1) as u64;
    let disc = 1 + 8 * u;
    let mut d = ((1 + disc.isqrt()) / 4).max(1) as u32;
    if diamond_ring_start(d) > index {
        d -= 1;
    } else if index >= diamond_ring_start(d + 1) {
        d += 1;
    }
    d
}

#[inline(always)]
fn index_to_xy_diamond_rings(index: u32) -> (i32, i32) {
    if index == 0 {
        return (0, 0);
    }
    let d = diamond_ring_for_index(index);
    let di = d as i32;
    let start = diamond_ring_start(d);
    let offset = index - start;
    let last = 4 * d - 1;
    if offset == 0 {
        return (-di, 0);
    }
    if offset == last {
        return (di, 0);
    }
    let o = offset - 1;
    let x_idx = (o / 2) as i32;
    let x = -di + 1 + x_idx;
    let rem = di - x.abs();
    let y = if o & 1 == 0 { rem } else { -rem };
    (x, y)
}

#[inline(always)]
fn xy_to_index_diamond_rings(x: i32, y: i32) -> u32 {
    let d = (x.abs() + y.abs()) as u32;
    let di = d as i32;
    let start = diamond_ring_start(d);
    if d == 0 {
        return 0;
    }
    if y == 0 && x == -di {
        return start;
    }
    if y == 0 && x == di {
        return start + 4 * d - 1;
    }
    let rem = di - x.abs();
    let within = if y == rem { 0 } else { 1 };
    let before_x = 1 + 2 * (x + di - 1) as u32;
    start + before_x + within
}

/// O(1) diamond-ring successor (no `index_to_xy`).
#[inline(always)]
fn diamond_scan_step((x, y): (i32, i32)) -> (i32, i32) {
    if x == 0 && y == 0 {
        return (-1, 0);
    }
    let d = x.abs() + y.abs();
    let rem = d - x.abs();
    if rem > 0 && y == rem {
        return (x, -rem);
    }
    if x < d {
        let nx = x + 1;
        let nrem = d - nx.abs();
        if nrem == 0 {
            return (nx, 0);
        }
        return (nx, nrem);
    }
    (-(d + 1), 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn assert_scan_walk(order: VisitOrder, limit: u32) {
        let mut xy = (0, 0);
        for index in 0..limit {
            assert_eq!(
                order.index_to_xy(index),
                xy,
                "{order:?} index {index}"
            );
            let at = xy;
            xy = order.scan_step_xy(index, xy);
            assert_eq!(
                order.index_to_xy(index.wrapping_add(1)),
                xy,
                "{order:?} after index {index} at {at:?}"
            );
        }
    }

    fn assert_round_trip(order: VisitOrder, limit: u32) {
        for index in 0..limit {
            let (x, y) = order.index_to_xy(index);
            assert_eq!(order.xy_to_index(x, y), index, "{order:?} index {index}");
        }
    }

    fn assert_unique_cells(order: VisitOrder, limit: u32) {
        let mut seen = HashSet::new();
        for index in 0..limit {
            let xy = order.index_to_xy(index);
            assert!(seen.insert(xy), "{order:?} duplicate {xy:?} at {index}");
        }
    }

    #[test]
    fn all_orders_scan_and_round_trip_near_origin() {
        for order in VisitOrder::ALL {
            assert_scan_walk(order, 20_000);
            assert_round_trip(order, 10_000);
            assert_unique_cells(order, 5_000);
        }
    }

    #[test]
    fn square_spiral_ccw_scan_and_round_trip() {
        assert_scan_walk(VisitOrder::SquareSpiral, 20_000);
        assert_round_trip(VisitOrder::SquareSpiral, 10_000);
    }

    #[test]
    fn square_spiral_cw_scan_and_round_trip() {
        assert_scan_walk(VisitOrder::SquareSpiralClockwise, 20_000);
        assert_round_trip(VisitOrder::SquareSpiralClockwise, 10_000);
    }

    #[test]
    fn square_rings_scanline_scan_and_round_trip() {
        assert_scan_walk(VisitOrder::SquareRingsScanline, 20_000);
        assert_round_trip(VisitOrder::SquareRingsScanline, 10_000);
    }

    #[test]
    fn diamond_rings_scan_and_round_trip() {
        assert_scan_walk(VisitOrder::DiamondRings, 20_000);
        assert_round_trip(VisitOrder::DiamondRings, 10_000);
    }

    #[test]
    fn visit_order_prev_next_cycles_all() {
        for (i, &order) in VisitOrder::ALL.iter().enumerate() {
            assert_eq!(order.prev().next(), order);
            assert_eq!(order.next().prev(), order);
            assert_eq!(order.next(), VisitOrder::ALL[(i + 1) % VisitOrder::ALL.len()]);
        }
    }

    #[test]
    fn clockwise_differs_from_ccw_after_first_steps() {
        assert_eq!(VisitOrder::SquareSpiral.index_to_xy(2), (1, 1));
        assert_eq!(VisitOrder::SquareSpiralClockwise.index_to_xy(2), (1, -1));
    }

    #[test]
    fn square_spiral_matches_spiral_module() {
        for index in 0..10_000 {
            assert_eq!(
                VisitOrder::SquareSpiral.index_to_xy(index),
                crate::spiral::index_to_xy(index)
            );
            let (x, y) = crate::spiral::index_to_xy(index);
            assert_eq!(VisitOrder::SquareSpiral.xy_to_index(x, y), index);
        }
    }
}
