//! Monotonic cell visit order: each `u32` index maps to one board cell `(x, y)`.
//!
//! The production [`crate::sim::Simulation`] calls [`crate::spiral`] on the hot path so
//! release timing matches the pre-strategy baseline. Implement [`IndexOrder`] for a new
//! mapping, then wire `index_to_xy` / `xy_to_index` / [`IndexOrder::scan_step_xy`] into a
//! fork of `Simulation::step_turn_scan` (or a bench-only copy) when experimenting.

use std::fmt;

/// How spiral indices map to the infinite square grid and advance during placement scans.
pub trait IndexOrder: Copy + Default + Send + Sync + 'static {
    /// Short label for logs, UI, or saved configs.
    const NAME: &'static str;

    fn index_to_xy(index: u32) -> (i32, i32);
    fn xy_to_index(x: i32, y: i32) -> u32;

    /// Coordinates at index `index + 1` after rejecting index `index` at `xy`.
    ///
    /// Scan loops already advance the numeric cursor; this only updates `(x, y)`.
    /// Default uses [`Self::index_to_xy`]; override when a local step exists (see [`SquareSpiral`]).
    fn scan_step_xy(index: u32, _xy: (i32, i32)) -> (i32, i32) {
        Self::index_to_xy(index.wrapping_add(1))
    }
}

/// Square counterclockwise spiral (OEIS A316667 geometry); production default.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SquareSpiral;

impl fmt::Display for SquareSpiral {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(Self::NAME)
    }
}

impl IndexOrder for SquareSpiral {
    const NAME: &'static str = "square_spiral";

    #[inline(always)]
    fn index_to_xy(index: u32) -> (i32, i32) {
        crate::spiral::index_to_xy(index)
    }

    #[inline(always)]
    fn xy_to_index(x: i32, y: i32) -> u32 {
        crate::spiral::xy_to_index(x, y)
    }

    #[inline(always)]
    fn scan_step_xy(_index: u32, xy: (i32, i32)) -> (i32, i32) {
        crate::spiral::spiral_step(xy)
    }
}

/// Active visit order for the crate. Change this alias to try another [`IndexOrder`] type.
pub type DefaultIndexOrder = SquareSpiral;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn square_spiral_scan_step_matches_index_to_xy() {
        let mut xy = (0, 0);
        for index in 0..20_000u32 {
            assert_eq!(SquareSpiral::index_to_xy(index), xy, "index {index}");
            xy = SquareSpiral::scan_step_xy(index, xy);
            assert_eq!(SquareSpiral::index_to_xy(index.wrapping_add(1)), xy);
        }
    }

    #[test]
    fn square_spiral_xy_round_trip() {
        for index in 0..10_000 {
            let (x, y) = SquareSpiral::index_to_xy(index);
            assert_eq!(SquareSpiral::xy_to_index(x, y), index);
        }
    }
}
