//! Counters and coarse timings for `Simulation::place` (feature `place_profile` only).

use bevy::platform::time::Instant;
use std::cell::{Cell, RefCell};

#[derive(Clone, Debug, Default)]
pub struct PlaceWorkStats {
    pub placements: u64,
    pub scan_cells_examined: u64,
    pub valid_move_steps: u64,
    pub xy_to_index_calls: u64,
    pub forbidden_bit_inserts: u64,
    pub occupancy_grows: u64,
    pub forbidden_word_grows: u64,
    /// How often `combined_forbidden_word` runs during scan (feature `place_profile`).
    pub scan_forb_word_combines: u64,
    /// Full forbidden-word tail jumps (`next..word_end` all forbidden).
    pub scan_forbidden_tail_skips: u64,
    /// Single-index advances (`spiral_step` path) after a rejection.
    pub scan_single_step_rejects: u64,
}

/// Per-`ForbiddenSet::insert` call during a profiled run.
#[derive(Clone, Debug, Default)]
pub struct ForbiddenInsertStats {
    pub calls: u64,
    pub or_into_existing_word: u64,
    pub grow_new_word: u64,
    /// Bit was already set; insert still ran (idempotent OR).
    pub bit_already_set: u64,
}

#[derive(Clone, Debug, Default)]
pub struct PlaceTimingStats {
    pub step_turn_ns: u64,
    pub place_total_ns: u64,
    pub occupancy_insert_ns: u64,
    pub record_forbidden_ns: u64,
    pub placements_push_ns: u64,
}

thread_local! {
    static WORK: RefCell<PlaceWorkStats> = RefCell::new(PlaceWorkStats::default());
    static FORB_INSERT: RefCell<ForbiddenInsertStats> =
        RefCell::new(ForbiddenInsertStats::default());
    static TIMING: RefCell<PlaceTimingStats> = RefCell::new(PlaceTimingStats::default());
    static TIMING_ENABLED: Cell<bool> = const { Cell::new(true) };
    static PROFILING_ENABLED: Cell<bool> = const { Cell::new(false) };
}

pub fn set_timing_enabled(enabled: bool) {
    TIMING_ENABLED.with(|f| f.set(enabled));
}

pub fn set_profiling_enabled(enabled: bool) {
    PROFILING_ENABLED.with(|f| f.set(enabled));
}

pub fn profiling_active() -> bool {
    profiling_enabled()
}

fn profiling_enabled() -> bool {
    PROFILING_ENABLED.with(|f| f.get())
}

pub fn timing_enabled_for_place() -> bool {
    timing_enabled()
}

fn timing_enabled() -> bool {
    TIMING_ENABLED.with(|f| f.get())
}

pub fn reset() {
    WORK.with(|w| *w.borrow_mut() = PlaceWorkStats::default());
    FORB_INSERT.with(|s| *s.borrow_mut() = ForbiddenInsertStats::default());
    TIMING.with(|t| *t.borrow_mut() = PlaceTimingStats::default());
}

pub fn take_work() -> PlaceWorkStats {
    WORK.with(|w| std::mem::take(&mut *w.borrow_mut()))
}

pub fn take_timing() -> PlaceTimingStats {
    TIMING.with(|t| std::mem::take(&mut *t.borrow_mut()))
}

pub fn add_scan_cells(n: u64) {
    if !profiling_enabled() {
        return;
    }
    WORK.with(|w| w.borrow_mut().scan_cells_examined += n);
}

pub fn note_scan_forb_word_combine() {
    if !profiling_enabled() {
        return;
    }
    WORK.with(|w| w.borrow_mut().scan_forb_word_combines += 1);
}

pub fn note_scan_forbidden_tail_skip() {
    if !profiling_enabled() {
        return;
    }
    WORK.with(|w| w.borrow_mut().scan_forbidden_tail_skips += 1);
}

pub fn note_scan_single_step_reject() {
    if !profiling_enabled() {
        return;
    }
    WORK.with(|w| w.borrow_mut().scan_single_step_rejects += 1);
}

pub fn note_placement_work(valid_moves: u64, targets: u64) {
    if !profiling_enabled() {
        return;
    }
    WORK.with(|w| {
        let mut s = w.borrow_mut();
        s.placements += 1;
        s.valid_move_steps += valid_moves;
        s.xy_to_index_calls += valid_moves;
        s.forbidden_bit_inserts += valid_moves * targets;
    });
}

pub fn note_occupancy_grow() {
    if !profiling_enabled() {
        return;
    }
    WORK.with(|w| w.borrow_mut().occupancy_grows += 1);
}

pub fn note_forbidden_word_grow() {
    if !profiling_enabled() {
        return;
    }
    WORK.with(|w| w.borrow_mut().forbidden_word_grows += 1);
}

pub fn take_forbidden_insert_stats() -> ForbiddenInsertStats {
    FORB_INSERT.with(|s| std::mem::take(&mut *s.borrow_mut()))
}

pub fn note_forbidden_or_existing_word(already_set: bool) {
    if !profiling_enabled() {
        return;
    }
    FORB_INSERT.with(|s| {
        let mut stats = s.borrow_mut();
        stats.calls += 1;
        stats.or_into_existing_word += 1;
        if already_set {
            stats.bit_already_set += 1;
        }
    });
}

pub fn note_forbidden_or_new_word() {
    if !profiling_enabled() {
        return;
    }
    FORB_INSERT.with(|s| {
        let mut stats = s.borrow_mut();
        stats.calls += 1;
        stats.grow_new_word += 1;
    });
    note_forbidden_word_grow();
}

pub fn time_step_turn<F: FnOnce() -> bool>(f: F) -> bool {
    if !timing_enabled() {
        return f();
    }
    let start = Instant::now();
    let ok = f();
    TIMING.with(|t| t.borrow_mut().step_turn_ns += start.elapsed().as_nanos() as u64);
    ok
}

pub fn time_occupancy_insert<F: FnOnce()>(f: F) {
    if !timing_enabled() {
        f();
        return;
    }
    let start = Instant::now();
    f();
    TIMING.with(|t| t.borrow_mut().occupancy_insert_ns += start.elapsed().as_nanos() as u64);
}

pub fn time_record_forbidden<F: FnOnce()>(f: F) {
    if !timing_enabled() {
        f();
        return;
    }
    let start = Instant::now();
    f();
    TIMING.with(|t| t.borrow_mut().record_forbidden_ns += start.elapsed().as_nanos() as u64);
}

pub fn time_placements_push<F: FnOnce()>(f: F) {
    if !timing_enabled() {
        f();
        return;
    }
    let start = Instant::now();
    f();
    TIMING.with(|t| t.borrow_mut().placements_push_ns += start.elapsed().as_nanos() as u64);
}

pub fn add_place_total_ns(n: u64) {
    if !timing_enabled() {
        return;
    }
    TIMING.with(|t| t.borrow_mut().place_total_ns += n);
}

// `(target_piece, spiral_index)` for each `forbid_index` call.
thread_local! {
    static FORBIDDEN_RECORDS: RefCell<Vec<(usize, u32)>> = RefCell::new(Vec::new());
}

pub fn clear_forbidden_records() {
    FORBIDDEN_RECORDS.with(|s| s.borrow_mut().clear());
}

pub fn push_forbidden_record(target_piece: usize, index: u32) {
    if !profiling_enabled() {
        return;
    }
    FORBIDDEN_RECORDS.with(|s| s.borrow_mut().push((target_piece, index)));
}

pub fn take_forbidden_records() -> Vec<(usize, u32)> {
    FORBIDDEN_RECORDS.with(|s| std::mem::take(&mut *s.borrow_mut()))
}

/// Mirrors `ForbiddenSet::insert` for isolated replay timing.
#[derive(Default)]
pub struct ForbiddenInsertHarness {
    words: Vec<u64>,
}

impl ForbiddenInsertHarness {
    pub fn insert(&mut self, index: u32) {
        let word_index = index as usize >> 6;
        let bit = 1u64 << (index & 63);
        if word_index < self.words.len() {
            self.words[word_index] |= bit;
        } else {
            self.words.resize(word_index + 1, 0);
            self.words[word_index] |= bit;
        }
    }
}

/// Mirrors `OccupancyGrid::insert` for isolated replay timing.
pub struct OccupancyInsertHarness {
    cells: Vec<usize>,
}

impl OccupancyInsertHarness {
    pub fn new() -> Self {
        Self { cells: Vec::new() }
    }

    pub fn insert(&mut self, index: u32, piece_id: usize) {
        const EMPTY: usize = usize::MAX;
        let index = index as usize;
        if index >= self.cells.len() {
            self.cells.resize(index + 1, EMPTY);
        }
        self.cells[index] = piece_id;
    }

    pub fn len(&self) -> usize {
        self.cells.len()
    }
}
