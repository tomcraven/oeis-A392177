use crate::model::{GameDefinition, MAX_PIECES, PieceId};

pub use crate::index_order::{IndexOrder, SquareSpiral, VisitOrder};
use bevy::prelude::{FromWorld, Resource, World};
use std::sync::Arc;
use std::time::Duration;

use bevy::platform::time::Instant;

const EMPTY_ARMY: PieceId = usize::MAX;
/// Sentinel for unoccupied spiral indices (shared with render).
pub(crate) const EMPTY_ARMY_SLOT: PieceId = EMPTY_ARMY;

/// Soft cap on the simulation's heap footprint (occupancy + placements + attack grid). Once a
/// placement would push past this, the sim stops advancing and renders what it has rather than
/// growing further. On wasm this keeps us clear of the linear-memory ceiling *including* the
/// transient copy `ensure_unique_for_mutation` makes of occupancy+placements; on native it is set
/// high and the fallible `try_reserve` growth is the real backstop. See [`Simulation::footprint_bytes`].
#[cfg(target_family = "wasm")]
pub const MEM_BUDGET_BYTES: usize = 1 << 30; // 1 GiB
#[cfg(not(target_family = "wasm"))]
pub const MEM_BUDGET_BYTES: usize = 12 << 30; // 12 GiB

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScanSkips {
    pub forbidden: Vec<u32>,
    pub occupied: Vec<u32>,
}

/// Cheap per-piece placement aggregates maintained in [`Simulation::place`] (once per placement,
/// never in the per-cell scan loop) so the Debug stats panel needs no replay. The number of cells
/// a piece has examined is its monotonic `cursor`, so skip counts derive from these without
/// touching the hot loop.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PieceTally {
    pub placements: u32,
    /// Spiral index of this piece's first placement (smallest, since cursors are monotonic).
    pub first_index: u32,
    /// Spiral index of this piece's latest placement (largest seen so far).
    pub last_index: u32,
}

fn resize_piece_vectors<T: Clone>(vec: &mut Vec<T>, len: usize, fill: T) {
    if vec.len() == len {
        vec.fill(fill);
    } else {
        *vec = vec![fill; len];
    }
}

#[derive(Resource)]
pub struct Simulation {
    pub visit_order: VisitOrder,
    /// Dense by spiral index because simulation placement scans are numeric and monotonic.
    /// This avoids hashing on every occupied-cell check in the hot loop.
    pub occupancy: OccupancyGrid,
    /// Cumulative attacked cells in board `(x, y)` space: each cell stores a bitmask of the
    /// attackers that hit it. Marking is a plain `row*stride+col` write (no `xy_to_index`),
    /// and defender `d`'s scan tests `attack_grid.at(x, y) & respected_mask[d]`.
    attack_grid: AttackGrid,
    /// For each defender piece, a bitmask (one bit per attacker id) of the attackers whose
    /// threats block its placement.
    respected_mask: Vec<u32>,
    /// Per piece, the max Chebyshev move radius — pre-grows the grid once per placement.
    move_radius: Vec<i32>,
    /// Enabled turn order captured with the definition-derived simulation metadata.
    active_turn_order: Vec<PieceId>,
    pub cursors: Vec<u32>,
    cursor_positions: Vec<(i32, i32)>,
    /// Rolling cursor into `turn_order`; avoids a modulo in every simulated turn.
    turn_order_index: usize,
    pub turn_step: usize,
    pub placements: PlacementsLog,
    /// Per-piece placement counts and spiral reach; updated once per placement (see [`PieceTally`]).
    piece_tally: Vec<PieceTally>,
    /// Set once a placement could not be admitted within `mem_budget_bytes` (or a real allocation
    /// failed). While saturated the sim refuses to advance, so the board renders the region filled
    /// so far instead of crashing. Cleared by [`Simulation::reset`].
    saturated: bool,
    /// Soft heap budget for the index-scaled structures; defaults to [`MEM_BUDGET_BYTES`].
    mem_budget_bytes: usize,
}

/// Append-only placement history; [`Arc`] snapshot for UI without cloning on every worker tick.
#[derive(Clone, Debug, Default)]
pub struct PlacementsLog {
    entries: Arc<Vec<(u32, PieceId)>>,
}

impl PlacementsLog {
    fn new() -> Self {
        Self {
            entries: Arc::new(Vec::new()),
        }
    }

    /// Split shared storage before mutating while the UI holds a snapshot [`Arc`].
    pub fn ensure_unique_for_mutation(&mut self) {
        if Arc::strong_count(&self.entries) > 1 {
            self.entries = Arc::new(self.entries.as_ref().clone());
        }
    }

    fn clear(&mut self) {
        Arc::make_mut(&mut self.entries).clear();
    }

    /// Append an entry, growing fallibly. Returns `false` on allocation failure (wasm OOM). The
    /// `try_reserve(1)` is a cheap capacity check on the steady-state path.
    fn push(&mut self, index: u32, piece_id: PieceId) -> bool {
        let entries = Arc::make_mut(&mut self.entries);
        // Only the (rare) grow is fallible; when spare capacity exists this is a plain push.
        if entries.len() == entries.capacity() && entries.try_reserve(1).is_err() {
            return false;
        }
        entries.push((index, piece_id));
        true
    }

    fn byte_capacity(&self) -> usize {
        self.entries.capacity() * std::mem::size_of::<(u32, PieceId)>()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn as_slice(&self) -> &[(u32, PieceId)] {
        self.entries.as_ref()
    }

    pub fn arc(&self) -> Arc<Vec<(u32, PieceId)>> {
        Arc::clone(&self.entries)
    }

    #[cfg(test)]
    fn capacity(&self) -> usize {
        self.entries.capacity()
    }
}

impl std::ops::Deref for PlacementsLog {
    type Target = [(u32, PieceId)];

    fn deref(&self) -> &Self::Target {
        self.entries.as_ref()
    }
}

impl PartialEq for PlacementsLog {
    fn eq(&self, other: &Self) -> bool {
        self.entries.as_ref() == other.entries.as_ref()
    }
}

impl PartialEq<Vec<(u32, PieceId)>> for PlacementsLog {
    fn eq(&self, other: &Vec<(u32, PieceId)>) -> bool {
        self.as_slice() == other.as_slice()
    }
}

impl PartialEq<PlacementsLog> for Vec<(u32, PieceId)> {
    fn eq(&self, other: &PlacementsLog) -> bool {
        other == self
    }
}

impl Simulation {
    pub fn new(def: &GameDefinition, visit_order: VisitOrder) -> Self {
        let respected_mask = respected_masks(def);
        let attack_grid = AttackGrid::new(cell_width_for(&respected_mask));
        Self {
            visit_order,
            occupancy: OccupancyGrid::new(),
            attack_grid,
            respected_mask,
            move_radius: move_radii(def),
            active_turn_order: active_turn_order(def),
            cursors: vec![0; def.pieces.len()],
            cursor_positions: vec![(0, 0); def.pieces.len()],
            turn_order_index: 0,
            turn_step: 0,
            placements: PlacementsLog::new(),
            piece_tally: vec![PieceTally::default(); def.pieces.len()],
            saturated: false,
            mem_budget_bytes: MEM_BUDGET_BYTES,
        }
    }

    pub fn reset(&mut self, def: &GameDefinition) {
        self.occupancy.clear();
        self.saturated = false;
        let piece_count = def.pieces.len();
        self.respected_mask = respected_masks(def);
        let width = cell_width_for(&self.respected_mask);
        if self.attack_grid.cells.width() == width {
            self.attack_grid.clear();
        } else {
            self.attack_grid = AttackGrid::new(width);
        }
        self.move_radius = move_radii(def);
        self.active_turn_order = active_turn_order(def);
        resize_piece_vectors(&mut self.cursors, piece_count, 0);
        resize_piece_vectors(&mut self.cursor_positions, piece_count, (0, 0));
        resize_piece_vectors(&mut self.piece_tally, piece_count, PieceTally::default());
        self.turn_order_index = 0;
        self.turn_step = 0;
        self.placements.clear();
    }

    /// Per-piece placement aggregates for the Debug stats panel. See [`PieceTally`].
    pub fn piece_tally(&self) -> &[PieceTally] {
        &self.piece_tally
    }

    /// Record one successful placement in the per-piece tally (called from [`Self::place`]).
    #[inline]
    fn note_placement_tally(&mut self, index: u32, piece_id: PieceId) {
        if let Some(t) = self.piece_tally.get_mut(piece_id) {
            if t.placements == 0 {
                t.first_index = index;
            }
            t.last_index = index;
            t.placements += 1;
        }
    }

    /// Set once the sim stops growing (budget reached or a real allocation failed). Callers should
    /// treat this like "no more work": stop requesting advances and render the current board.
    pub fn is_saturated(&self) -> bool {
        self.saturated
    }

    /// Override the soft heap budget (bytes) used to stop advancing. Mainly for tests and tuning;
    /// production uses [`MEM_BUDGET_BYTES`]. Does not retroactively clear an existing saturation.
    pub fn set_memory_budget_bytes(&mut self, budget: usize) {
        self.mem_budget_bytes = budget;
    }

    pub fn memory_budget_bytes(&self) -> usize {
        self.mem_budget_bytes
    }

    /// Approximate live heap held by the sim's index-scaled structures, used for the soft budget.
    /// Cheap (three capacity reads, no atomics).
    pub fn footprint_bytes(&self) -> usize {
        self.occupancy.byte_capacity()
            + self.placements.byte_capacity()
            + self.attack_grid.byte_capacity()
    }

    /// Soft-budget checkpoint: returns `true` (latching `saturated`) once the footprint reaches the
    /// budget. Called from the advance loops at their existing batch cadence — not per placement —
    /// so it is effectively free; the fallible allocations are the hard backstop in between.
    pub fn mem_saturated(&mut self) -> bool {
        if self.saturated {
            return true;
        }
        if self.footprint_bytes() >= self.mem_budget_bytes {
            self.saturated = true;
            return true;
        }
        false
    }

    /// Commit a placement. Each of the three index-scaled structures grows *fallibly* (the
    /// `try_reserve` is folded into its existing `Arc::make_mut`, so the common no-growth path adds
    /// nothing). Returns `false` and marks `saturated` if any allocation fails (e.g. wasm OOM), in
    /// which case the board renders the region filled so far instead of aborting.
    fn place(&mut self, def: &GameDefinition, index: u32, xy: (i32, i32), piece_id: PieceId) -> bool {
        #[cfg(feature = "place_profile")]
        if crate::place_profile::profiling_active() {
            let moves = def.piece(piece_id).piece.valid_moves.len() as u64;
            crate::place_profile::note_placement_work(moves, 1);
            let place_start = crate::place_profile::timing_enabled_for_place().then(Instant::now);
            crate::place_profile::time_occupancy_insert(|| {
                let _ = self.occupancy.insert(index, piece_id);
            });
            crate::place_profile::time_record_forbidden(|| {
                let _ = self.record_forbidden(def, xy, piece_id);
            });
            crate::place_profile::time_placements_push(|| {
                let _ = self.placements.push(index, piece_id);
            });
            if let Some(place_start) = place_start {
                crate::place_profile::add_place_total_ns(place_start.elapsed().as_nanos() as u64);
            }
            self.note_placement_tally(index, piece_id);
            return true;
        }
        let ok = self.occupancy.insert(index, piece_id)
            && self.record_forbidden(def, xy, piece_id)
            && self.placements.push(index, piece_id);
        if ok {
            self.note_placement_tally(index, piece_id);
        } else {
            self.saturated = true;
        }
        ok
    }

    fn record_forbidden(&mut self, def: &GameDefinition, xy: (i32, i32), piece_id: PieceId) -> bool {
        let moves = &def.piece(piece_id).piece.valid_moves;
        // One bit per attacker id; mark every attacked cell directly in coordinate space.
        // No `xy_to_index` per move — just a `row*stride+col` write into the grid.
        let bit = if piece_id < MAX_PIECES {
            1u32 << piece_id
        } else {
            0
        };
        if !self
            .attack_grid
            .record(xy.0, xy.1, bit, self.move_radius[piece_id], moves)
        {
            return false;
        }

        #[cfg(feature = "place_profile")]
        if crate::place_profile::profiling_active() {
            let (x, y) = xy;
            for &(dx, dy) in moves {
                let attacked = self.visit_order.xy_to_index(x + dx, y + dy);
                crate::place_profile::push_forbidden_record(piece_id, attacked);
            }
        }
        true
    }

    /// One piece takes a turn: scan from its cursor for the first legal square.
    pub fn step_turn(&mut self, def: &GameDefinition) -> bool {
        self.step_turn_scan::<false>(def, &mut 0)
    }

    pub fn active_turn_order_len(&self) -> usize {
        self.active_turn_order.len()
    }

    /// Piece id that will scan on the next [`Self::step_turn`].
    pub fn upcoming_piece_id(&self) -> PieceId {
        self.active_turn_order[self.turn_order_index]
    }

    /// Cells the upcoming piece skips on its next scan: attacked-but-empty vs already occupied.
    pub fn scan_skips_on_next_scan(&self, _def: &GameDefinition) -> ScanSkips {
        let mut forbidden = Vec::new();
        let mut occupied = Vec::new();
        let turn_order_len = self.active_turn_order.len();
        if turn_order_len == 0 {
            return ScanSkips { forbidden, occupied };
        }
        let piece_id = self.active_turn_order[self.turn_order_index];
        let respected_mask = self.respected_mask[piece_id];
        let mut cursor = self.cursors[piece_id];
        let mut xy = self.cursor_positions[piece_id];

        loop {
            let occupied_here = self.occupancy.contains_index(cursor);
            let forbidden_here = self.attack_grid.at(xy.0, xy.1) & respected_mask != 0;
            if !occupied_here && !forbidden_here {
                break;
            }
            if occupied_here {
                occupied.push(cursor);
            } else {
                forbidden.push(cursor);
            }

            let next = cursor.wrapping_add(1);
            if next == 0 {
                break;
            }
            cursor = next;
            xy = self.visit_order.scan_step_xy(cursor - 1, xy);
        }
        ScanSkips { forbidden, occupied }
    }

    /// Spiral cells rejected as forbidden (not occupied) on the next `step_turn` scan for the upcoming piece.
    pub fn forbidden_skips_on_next_scan(&self, def: &GameDefinition) -> Vec<u32> {
        self.scan_skips_on_next_scan(def).forbidden
    }

    /// Respected attackers whose cumulative attacks cover `index` during `scanning_piece`'s scan.
    pub fn respected_forbidden_attackers(&self, scanning_piece: PieceId, index: u32) -> Vec<PieceId> {
        let Some(&mask) = self.respected_mask.get(scanning_piece) else {
            return Vec::new();
        };
        let (x, y) = self.visit_order.index_to_xy(index);
        let hit = self.attack_grid.at(x, y) & mask;
        if hit == 0 {
            return Vec::new();
        }
        (0..self.respected_mask.len())
            .filter(|&attacker| hit & (1u32 << attacker) != 0)
            .collect()
    }

    /// Latest placement of `attacker` in this state's history whose move pattern hits `target_index`.
    pub fn placement_blocking_attacker(
        &self,
        def: &GameDefinition,
        attacker: PieceId,
        target_index: u32,
    ) -> Option<u32> {
        for &(from_index, pid) in self.placements.as_slice().iter().rev() {
            if pid != attacker {
                continue;
            }
            if placement_attacks_index(def, self.visit_order, from_index, attacker, target_index) {
                return Some(from_index);
            }
        }
        None
    }

    /// Like `step_turn`, but accumulates scan/place timings (requires feature `place_profile`).
    #[cfg(feature = "place_profile")]
    pub fn step_turn_profiled(&mut self, def: &GameDefinition) -> bool {
        crate::place_profile::time_step_turn(|| {
            let mut cells = 0u32;
            let ok = self.step_turn_scan::<true>(def, &mut cells);
            crate::place_profile::add_scan_cells(cells as u64);
            ok
        })
    }

    /// Re-run `place` for profiling replays (requires feature `place_profile`).
    #[cfg(feature = "place_profile")]
    pub fn replay_place_profiled(
        &mut self,
        def: &GameDefinition,
        index: u32,
        xy: (i32, i32),
        piece_id: PieceId,
    ) {
        self.place(def, index, xy, piece_id);
    }

    fn step_turn_scan<const COUNT_CELLS: bool>(
        &mut self,
        def: &GameDefinition,
        cells_examined: &mut u32,
    ) -> bool {
        let turn_order_len = self.active_turn_order.len();
        if turn_order_len == 0 {
            return false;
        }
        let piece_id = self.active_turn_order[self.turn_order_index];
        self.turn_order_index += 1;
        if self.turn_order_index == turn_order_len {
            self.turn_order_index = 0;
        }
        self.turn_step += 1;

        let respected_mask = self.respected_mask[piece_id];
        // Locals avoid re-indexing `cursors`/`cursor_positions` on every scanned cell.
        let mut cursor = self.cursors[piece_id];
        let mut xy = self.cursor_positions[piece_id];

        loop {
            if COUNT_CELLS {
                *cells_examined += 1;
            }
            let occupied = self.occupancy.contains_index(cursor);
            // Forbidden membership is a single coordinate-grid read masked by the
            // attackers this piece respects — no spiral-word OR per scanned cell.
            let forbidden_here = self.attack_grid.at(xy.0, xy.1) & respected_mask != 0;
            if !occupied && !forbidden_here {
                // Commit the placement. If an allocation fails (the hard backstop — e.g. wasm
                // OOM), `place` marks the sim saturated and returns false; we leave the cursor on
                // this cell and report no progress so the board renders what is already filled
                // instead of aborting. The *soft* memory budget is enforced at the advance-loop
                // checkpoints (see `mem_saturated`), keeping the per-placement path free of it.
                if !self.place(def, cursor, xy, piece_id) {
                    self.cursors[piece_id] = cursor;
                    self.cursor_positions[piece_id] = xy;
                    return false;
                }
                // Advance past the cell we just occupied so the next scan for this piece
                // doesn't waste an iteration confirming a self-placed occupied cell.
                self.cursors[piece_id] = cursor.saturating_add(1);
                self.cursor_positions[piece_id] = self.visit_order.scan_step_xy(cursor, xy);
                return true;
            }

            let next = cursor.wrapping_add(1);
            if next == 0 {
                self.cursors[piece_id] = cursor;
                self.cursor_positions[piece_id] = xy;
                return false;
            }
            cursor = next;
            xy = self.visit_order.scan_step_xy(cursor - 1, xy);
            #[cfg(feature = "place_profile")]
            crate::place_profile::note_scan_single_step_reject();
        }
    }

    pub fn needs_work(&self, def: &GameDefinition, target_index: u32) -> bool {
        if self.cursors.is_empty() {
            return false;
        }
        self.cursors.iter().enumerate().any(|(id, &c)| {
            def.pieces
                .get(id)
                .is_some_and(|a| a.enabled && c <= target_index)
        })
    }

    pub fn advance_to_target(&mut self, def: &GameDefinition, target_index: u32) {
        if self.saturated || def.pieces.is_empty() || self.active_turn_order.is_empty() {
            return;
        }
        let mut turns_since_check = 0u32;
        while self.needs_work(def, target_index) {
            if !self.step_turn(def) {
                break;
            }
            turns_since_check += 1;
            if turns_since_check == 4_096 {
                if self.mem_saturated() {
                    break;
                }
                turns_since_check = 0;
            }
        }
    }

    pub fn advance_for_duration(
        &mut self,
        def: &GameDefinition,
        target_index: u32,
        max_duration: Duration,
    ) {
        if self.saturated || def.pieces.is_empty() || self.active_turn_order.is_empty() {
            return;
        }
        self.occupancy.ensure_unique_for_mutation();
        self.placements.ensure_unique_for_mutation();
        let start = Instant::now();
        let mut turns_since_check = 0u32;
        while self.needs_work(def, target_index) {
            if !self.step_turn(def) {
                break;
            }
            turns_since_check += 1;

            // Checking the clock every turn costs too much in this hot loop.
            // Batch the check so the UI still updates while simulation uses most of
            // the allotted frame time.
            if turns_since_check == 4_096 {
                if start.elapsed() >= max_duration || self.mem_saturated() {
                    break;
                }
                turns_since_check = 0;
            }
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct OccupancyGrid {
    cells: Arc<Vec<PieceId>>,
}

impl OccupancyGrid {
    fn new() -> Self {
        Self {
            cells: Arc::new(Vec::new()),
        }
    }

    /// Split shared backing storage so the sim can mutate while UI holds a snapshot `Arc`.
    pub fn ensure_unique_for_mutation(&mut self) {
        if Arc::strong_count(&self.cells) > 1 {
            self.cells = Arc::new(self.cells.as_ref().clone());
        }
    }

    fn clear(&mut self) {
        Arc::make_mut(&mut self.cells).clear();
    }

    /// Place `piece_id` at spiral `index`, growing the dense grid as needed. Returns `false` if the
    /// grow allocation fails (wasm OOM). The fallible `try_reserve` only does work when the grid
    /// must actually grow, so the steady-state path matches the old infallible `resize`.
    fn insert(&mut self, index: u32, piece_id: PieceId) -> bool {
        let cells = Arc::make_mut(&mut self.cells);
        let index = index as usize;
        if index >= cells.len() {
            #[cfg(feature = "place_profile")]
            crate::place_profile::note_occupancy_grow();
            // Only fallibly reserve when the resize would actually reallocate (index past
            // capacity); otherwise the resize just fills spare capacity, as before.
            if index >= cells.capacity() && cells.try_reserve(index + 1 - cells.len()).is_err() {
                return false;
            }
            cells.resize(index + 1, EMPTY_ARMY);
        }
        cells[index] = piece_id;
        true
    }

    fn byte_capacity(&self) -> usize {
        self.cells.capacity() * std::mem::size_of::<PieceId>()
    }

    pub fn get(&self, index: &u32) -> Option<&PieceId> {
        let piece_id = self.cells.get(*index as usize)?;
        (*piece_id != EMPTY_ARMY).then_some(piece_id)
    }

    /// Hot-path lookup for rendering (spiral index → piece).
    pub fn piece_id_at(&self, index: u32) -> Option<PieceId> {
        let piece_id = *self.cells.get(index as usize)?;
        (piece_id != EMPTY_ARMY).then_some(piece_id)
    }

    pub fn index_of_piece(&self, piece_id: PieceId) -> Option<u32> {
        self.cells_slice()
            .iter()
            .enumerate()
            .find_map(|(index, &occupant)| (occupant == piece_id).then_some(index as u32))
    }

    pub fn cells_slice(&self) -> &[PieceId] {
        self.cells.as_ref()
    }

    fn contains_index(&self, index: u32) -> bool {
        self.cells
            .get(index as usize)
            .copied()
            .unwrap_or(EMPTY_ARMY)
            != EMPTY_ARMY
    }
}

/// Cell width for [`AttackGrid`]. Chosen from the highest *respected* attacker bit any defender
/// tests, so small rosters (every preset) store one byte per cell instead of four.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CellWidth {
    U8,
    U16,
    U32,
}

/// Smallest cell width that holds every bit a scan might test (`union` of all `respected_mask`s).
/// Bits a non-respected attacker writes either fall below the width (harmless) or truncate to 0
/// in [`AttackGrid::record`] — they are never read, so narrowing never changes scan results.
fn cell_width_for(respected_mask: &[u32]) -> CellWidth {
    let union = respected_mask.iter().fold(0u32, |acc, &m| acc | m);
    if union < (1 << 8) {
        CellWidth::U8
    } else if union < (1 << 16) {
        CellWidth::U16
    } else {
        CellWidth::U32
    }
}

/// Backing store for [`AttackGrid`]: a flat row-major grid in one of three integer widths. The
/// variant is fixed for a sim's lifetime, so the per-call `match` is a perfectly-predicted branch.
#[derive(Clone, Debug)]
enum MaskCells {
    U8(Vec<u8>),
    U16(Vec<u16>),
    U32(Vec<u32>),
}

impl MaskCells {
    fn zeros(width: CellWidth, len: usize) -> Self {
        match width {
            CellWidth::U8 => MaskCells::U8(vec![0; len]),
            CellWidth::U16 => MaskCells::U16(vec![0; len]),
            CellWidth::U32 => MaskCells::U32(vec![0; len]),
        }
    }

    fn width(&self) -> CellWidth {
        match self {
            MaskCells::U8(_) => CellWidth::U8,
            MaskCells::U16(_) => CellWidth::U16,
            MaskCells::U32(_) => CellWidth::U32,
        }
    }

    fn fill_zero(&mut self) {
        match self {
            MaskCells::U8(v) => v.fill(0),
            MaskCells::U16(v) => v.fill(0),
            MaskCells::U32(v) => v.fill(0),
        }
    }

    fn byte_capacity(&self) -> usize {
        match self {
            MaskCells::U8(v) => v.capacity(),
            MaskCells::U16(v) => v.capacity() * 2,
            MaskCells::U32(v) => v.capacity() * 4,
        }
    }

    #[cfg(test)]
    fn capacity(&self) -> usize {
        match self {
            MaskCells::U8(v) => v.capacity(),
            MaskCells::U16(v) => v.capacity(),
            MaskCells::U32(v) => v.capacity(),
        }
    }
}

/// Like [`regrow_cells`] but fallible: returns `None` if the larger buffer cannot be allocated
/// (wasm OOM) so [`AttackGrid::try_grow_to`] can stop instead of aborting.
fn try_regrow_cells<T: Copy>(
    old: &[T],
    old_half: i32,
    old_stride: usize,
    new_half: i32,
    new_stride: usize,
    zero: T,
) -> Option<Vec<T>> {
    let len = new_stride * new_stride;
    let mut new = Vec::new();
    new.try_reserve_exact(len).ok()?;
    new.resize(len, zero);
    let col_shift = (new_half - old_half) as usize;
    for y in -old_half..=old_half {
        let src_row = (y + old_half) as usize * old_stride;
        let dst_row = (y + new_half) as usize * new_stride + col_shift;
        new[dst_row..dst_row + old_stride].copy_from_slice(&old[src_row..src_row + old_stride]);
    }
    Some(new)
}

/// Cumulative attacked cells in board `(x, y)` space. Each cell holds a bitmask of the
/// attacker ids that hit it, so a defender's scan tests `at(x, y) & respected_mask` with a
/// single masked read. Marking takes a plain `row*stride+col` write — no `xy_to_index`.
#[derive(Clone, Debug)]
struct AttackGrid {
    /// Grid covers `[-half, half]` on both axes.
    half: i32,
    /// Row stride, `2 * half + 1`.
    stride: usize,
    /// `cells[(y + half) * stride + (x + half)]` = bitmask of attackers hitting `(x, y)`.
    cells: MaskCells,
}

impl Default for AttackGrid {
    fn default() -> Self {
        Self::new(CellWidth::U32)
    }
}

impl AttackGrid {
    fn new(width: CellWidth) -> Self {
        let half = 8i32;
        let stride = (2 * half + 1) as usize;
        Self {
            half,
            stride,
            cells: MaskCells::zeros(width, stride * stride),
        }
    }

    /// Reuse the allocation; clears all attacker bits (preset reload, same width).
    fn clear(&mut self) {
        self.cells.fill_zero();
    }

    #[inline]
    fn at(&self, x: i32, y: i32) -> u32 {
        if x > self.half || x < -self.half || y > self.half || y < -self.half {
            return 0;
        }
        let i = (y + self.half) as usize * self.stride + (x + self.half) as usize;
        match &self.cells {
            MaskCells::U8(v) => v[i] as u32,
            MaskCells::U16(v) => v[i] as u32,
            MaskCells::U32(v) => v[i],
        }
    }

    /// Mark every cell attacked by a piece (bit `bit`, max move radius `max_radius`) placed at
    /// `(px, py)`. One `abs().max()` per placement checks the grid extent; the width `match` is
    /// hoisted out of the move loop so marks stay branch-free per cell. Returns `false` if a
    /// required grow allocation fails (wasm OOM).
    #[inline]
    fn record(&mut self, px: i32, py: i32, bit: u32, max_radius: i32, moves: &[(i32, i32)]) -> bool {
        let reach = px.abs().max(py.abs()) + max_radius;
        if reach > self.half && !self.try_grow_to(reach) {
            return false;
        }
        let stride = self.stride as isize;
        let base = (py + self.half) as isize * stride + (px + self.half) as isize;
        match &mut self.cells {
            MaskCells::U8(v) => {
                let bit = bit as u8;
                for &(dx, dy) in moves {
                    let i = (base + dy as isize * stride + dx as isize) as usize;
                    v[i] |= bit;
                }
            }
            MaskCells::U16(v) => {
                let bit = bit as u16;
                for &(dx, dy) in moves {
                    let i = (base + dy as isize * stride + dx as isize) as usize;
                    v[i] |= bit;
                }
            }
            MaskCells::U32(v) => {
                for &(dx, dy) in moves {
                    let i = (base + dy as isize * stride + dx as isize) as usize;
                    v[i] |= bit;
                }
            }
        }
        true
    }

    /// Grow (doubling to amortise) to cover `[-need, need]`, returning `false` if the larger buffer
    /// cannot be allocated. A no-op (returns `true`) when `need` already fits, so it is cheap to
    /// call before every placement.
    #[cold]
    fn try_grow_to(&mut self, need: i32) -> bool {
        if need <= self.half {
            return true;
        }
        let new_half = (self.half * 2).max(need + 1);
        let new_stride = (2 * new_half + 1) as usize;
        let new_cells = match &self.cells {
            MaskCells::U8(v) => {
                try_regrow_cells(v, self.half, self.stride, new_half, new_stride, 0u8)
                    .map(MaskCells::U8)
            }
            MaskCells::U16(v) => {
                try_regrow_cells(v, self.half, self.stride, new_half, new_stride, 0u16)
                    .map(MaskCells::U16)
            }
            MaskCells::U32(v) => {
                try_regrow_cells(v, self.half, self.stride, new_half, new_stride, 0u32)
                    .map(MaskCells::U32)
            }
        };
        match new_cells {
            Some(cells) => {
                self.half = new_half;
                self.stride = new_stride;
                self.cells = cells;
                true
            }
            None => false,
        }
    }

    fn byte_capacity(&self) -> usize {
        self.cells.byte_capacity()
    }
}

/// Per defender, a bitmask of the attacker ids whose threats block its placement.
fn respected_masks(def: &GameDefinition) -> Vec<u32> {
    let piece_count = def.pieces.len();
    debug_assert!(
        piece_count <= MAX_PIECES,
        "piece count {piece_count} exceeds MAX_PIECES ({MAX_PIECES})"
    );
    let mut masks = vec![0u32; piece_count];
    for defender in 0..piece_count {
        for &attacker in &def.piece(defender).blocked_by {
            if attacker < MAX_PIECES {
                masks[defender] |= 1u32 << attacker;
            }
        }
    }
    masks
}

/// Per piece, the max Chebyshev radius of its move set (`0` when it has no moves).
fn move_radii(def: &GameDefinition) -> Vec<i32> {
    def.pieces
        .iter()
        .map(|piece| {
            piece
                .piece
                .valid_moves
                .iter()
                .map(|&(dx, dy)| dx.abs().max(dy.abs()))
                .max()
                .unwrap_or(0)
        })
        .collect()
}

fn active_turn_order(def: &GameDefinition) -> Vec<PieceId> {
    def.active_turn_order().iter().collect()
}

pub(crate) fn placement_attacks_index(
    def: &GameDefinition,
    visit_order: VisitOrder,
    from_index: u32,
    attacker: PieceId,
    target_index: u32,
) -> bool {
    let (x, y) = visit_order.index_to_xy(from_index);
    def.piece(attacker)
        .piece
        .valid_moves
        .iter()
        .any(|&(dx, dy)| visit_order.xy_to_index(x + dx, y + dy) == target_index)
}

#[cfg(test)]
fn threatened_for(def: &GameDefinition) -> Vec<Vec<PieceId>> {
    let mut threatened_for = vec![Vec::new(); def.pieces.len()];
    for target_piece in 0..def.pieces.len() {
        for &attacker in &def.piece(target_piece).blocked_by {
            if attacker < threatened_for.len() {
                threatened_for[attacker].push(target_piece);
            }
        }
    }
    threatened_for
}

impl FromWorld for Simulation {
    fn from_world(world: &mut World) -> Self {
        let def = world.resource::<GameDefinition>();
        Simulation::new(def, VisitOrder::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::GameDefinition;
    use crate::spiral::{index_to_xy, xy_to_index};
    use std::collections::HashSet;

    const GOLDEN_TURNS: [usize; 3] = [64, 1_024, 10_000];

    struct GoldenCase {
        name: &'static str,
        def: fn() -> GameDefinition,
        checksums: [u64; 3],
    }

    fn placement_checksum(placements: &[(u32, PieceId)]) -> u64 {
        let mut hash = 0xcbf2_9ce4_8422_2325u64;
        for &(index, piece_id) in placements {
            let value = ((index as u64) << 8) ^ piece_id as u64;
            hash ^= value;
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        hash
    }

    fn run_turns(def: &GameDefinition, turns: usize) -> Simulation {
        let mut sim = Simulation::new(def, VisitOrder::default());
        for _ in 0..turns {
            assert!(sim.step_turn(def));
        }
        sim
    }

    /// Rejected cells before placement on a successful turn (`cells_examined - 1`).
    fn rejections_on_success(cells_examined: u32) -> u32 {
        cells_examined.saturating_sub(1)
    }

    #[derive(Debug)]
    struct RejectionStats {
        turns_sampled: usize,
        max_rejections: u32,
        max_rejections_turn: usize,
        p50_rejections: u32,
        p99_rejections: u32,
        mean_rejections: f64,
    }

    fn collect_rejection_stats(
        def: &GameDefinition,
        total_turns: usize,
        late_window: usize,
    ) -> RejectionStats {
        let mut sim = Simulation::new(def, VisitOrder::default());
        let mut late_rejections = Vec::with_capacity(late_window.min(total_turns));
        let mut max_rejections = 0u32;
        let mut max_rejections_turn = 0usize;
        let mut sum = 0u64;

        for turn in 0..total_turns {
            let mut cells_examined = 0u32;
            let placed = sim.step_turn_scan::<true>(def, &mut cells_examined);
            assert!(placed, "turn {turn} failed to place");
            let examined = cells_examined;
            let rejections = rejections_on_success(examined);
            sum += rejections as u64;
            if rejections > max_rejections {
                max_rejections = rejections;
                max_rejections_turn = turn + 1;
            }
            if turn + late_window >= total_turns {
                late_rejections.push(rejections);
            }
        }

        late_rejections.sort_unstable();
        let p50 = late_rejections[late_rejections.len() / 2];
        let p99_idx = ((late_rejections.len() as f64) * 0.99).floor() as usize;
        let p99 = late_rejections[p99_idx.min(late_rejections.len().saturating_sub(1))];

        RejectionStats {
            turns_sampled: total_turns,
            max_rejections,
            max_rejections_turn,
            p50_rejections: p50,
            p99_rejections: p99,
            mean_rejections: sum as f64 / total_turns as f64,
        }
    }

    fn format_rejection_stats(label: &str, stats: &RejectionStats, late_window: usize) -> String {
        format!(
            "{label}: turns={} late_last={} max_rej={} @turn{} late_p50={} late_p99={} mean_rej={:.1}",
            stats.turns_sampled,
            late_window,
            stats.max_rejections,
            stats.max_rejections_turn,
            stats.p50_rejections,
            stats.p99_rejections,
            stats.mean_rejections,
        )
    }

    /// How many spiral cells are rejected before each placement in late game?
    /// Run with `cargo testd scan_rejection -- --nocapture` to print the report.
    #[test]
    fn scan_rejection_late_game_presets_and_random() {
        use crate::random_gen::{AttackSymmetry, RandomGenConfig, generate_random_game};
        use rand::SeedableRng;
        use rand::rngs::StdRng;

        const PRESET_TURNS: usize = 100_000;
        const RANDOM_TURNS: usize = 20_000;
        const LATE_WINDOW: usize = 1_000;
        // The coordinate-grid forbidden representation has no spiral-word structure, so the
        // forbidden word-tail skip is gone: a long forbidden run is now single-stepped (one
        // examined cell each) rather than jumped as a single iteration. Placements are
        // identical (golden checksums hold); worst-case examined cells per turn are therefore
        // higher than the pre-grid ~286, but each cell is a cheap masked grid read. This guard
        // just bounds that worst case well below a "thousands per turn" regime that would
        // motivate a successor/rank-select structure.
        const MAX_REJECTIONS_BOUND: u32 = 2_000;

        let mut global_max = 0u32;
        let mut global_max_label = String::new();

        let preset_cases: [(&str, fn() -> GameDefinition); 5] = [
            ("knight_2_pairwise", GameDefinition::knight_2_pairwise),
            ("knight_3_clique", GameDefinition::knight_3_clique),
            (
                "leaper_4_mixed_clique",
                GameDefinition::leaper_4_mixed_clique,
            ),
            ("king_6_clique", GameDefinition::king_6_clique),
            ("chimera_3_clique", GameDefinition::chimera_3_clique),
        ];

        eprintln!("\n=== scan rejections (rejections = cells examined - 1 on success) ===");
        for (name, def_fn) in preset_cases {
            let def = def_fn();
            let stats = collect_rejection_stats(&def, PRESET_TURNS, LATE_WINDOW);
            eprintln!("{}", format_rejection_stats(name, &stats, LATE_WINDOW));
            if stats.max_rejections > global_max {
                global_max = stats.max_rejections;
                global_max_label = name.to_string();
            }
        }

        let random_configs: [(&str, RandomGenConfig); 4] = [
            ("random_default", RandomGenConfig::default()),
            (
                "random_dense_clique_like",
                RandomGenConfig {
                    piece_count_min: 4,
                    piece_count_max: 6,
                    attack_radius_min: 1,
                    attack_radius_max: 3,
                    pattern_density: 0.55,
                    attack_symmetry: AttackSymmetry::Both,
                    identical_pieces: false,
                },
            ),
            (
                "random_sparse",
                RandomGenConfig {
                    piece_count_min: 2,
                    piece_count_max: 4,
                    attack_radius_min: 2,
                    attack_radius_max: 4,
                    pattern_density: 0.15,
                    attack_symmetry: AttackSymmetry::None,
                    identical_pieces: false,
                },
            ),
            (
                "random_wide_attacks",
                RandomGenConfig {
                    piece_count_min: 3,
                    piece_count_max: 5,
                    attack_radius_min: 3,
                    attack_radius_max: 5,
                    pattern_density: 0.45,
                    attack_symmetry: AttackSymmetry::Vertical,
                    identical_pieces: false,
                },
            ),
        ];

        for (cfg_name, mut cfg) in random_configs {
            cfg.sanitize();
            for seed in 0..3u64 {
                let mut rng = StdRng::seed_from_u64(seed);
                let def = generate_random_game(&cfg, &mut rng);
                let label = format!("{cfg_name}_seed{seed}_pieces{}", def.pieces.len());
                let stats =
                    collect_rejection_stats(&def, RANDOM_TURNS, LATE_WINDOW.min(RANDOM_TURNS));
                eprintln!(
                    "{}",
                    format_rejection_stats(&label, &stats, LATE_WINDOW.min(RANDOM_TURNS))
                );
                if stats.max_rejections > global_max {
                    global_max = stats.max_rejections;
                    global_max_label = label;
                }
            }
        }

        eprintln!(
            "=== overall max rejections: {global_max} ({global_max_label}); \
             bound {MAX_REJECTIONS_BOUND}: {}",
            if global_max >= MAX_REJECTIONS_BOUND {
                "EXCEEDED"
            } else {
                "ok"
            }
        );

        assert!(
            global_max < MAX_REJECTIONS_BOUND,
            "scan worst case grew unexpectedly large; \
             got max {global_max} on {global_max_label} (bound {MAX_REJECTIONS_BOUND})"
        );
    }

    fn assert_valid_placements(def: &GameDefinition, placements: &[(u32, PieceId)]) {
        let threatened_for = threatened_for(def);
        let mut occupied = HashSet::new();
        let mut forbidden = vec![HashSet::new(); def.pieces.len()];

        for &(index, piece_id) in placements {
            assert!(piece_id < def.pieces.len(), "invalid piece id {piece_id}");
            assert!(occupied.insert(index), "duplicate placement at {index}");
            assert!(
                !forbidden[piece_id].contains(&index),
                "piece {piece_id} placed on forbidden square {index}"
            );

            let xy = index_to_xy(index);
            for target_piece in 0..forbidden.len() {
                forbidden[target_piece].insert(index);
            }
            for &target_piece in &threatened_for[piece_id] {
                for &(dx, dy) in &def.piece(piece_id).piece.valid_moves {
                    forbidden[target_piece].insert(xy_to_index(xy.0 + dx, xy.1 + dy));
                }
            }
        }
    }

    fn golden_cases() -> [GoldenCase; 5] {
        [
            GoldenCase {
                name: "knight_2_pairwise",
                def: GameDefinition::knight_2_pairwise,
                checksums: [
                    15_737_156_276_822_775_461,
                    5_149_276_635_673_381_925,
                    561_431_110_996_648_581,
                ],
            },
            GoldenCase {
                name: "knight_3_clique",
                def: GameDefinition::knight_3_clique,
                checksums: [
                    16_115_999_991_126_781_684,
                    10_088_445_098_850_287_540,
                    7_584_768_825_753_057_092,
                ],
            },
            GoldenCase {
                name: "leaper_4_mixed_clique",
                def: GameDefinition::leaper_4_mixed_clique,
                checksums: [
                    5_964_283_847_930_621_157,
                    6_946_720_379_821_596_453,
                    6_370_614_915_775_779_925,
                ],
            },
            GoldenCase {
                name: "king_6_clique",
                def: GameDefinition::king_6_clique,
                checksums: [
                    6_480_521_862_097_834_845,
                    15_603_942_777_120_392_349,
                    1_643_601_116_650_407_053,
                ],
            },
            GoldenCase {
                name: "chimera_3_clique",
                def: GameDefinition::chimera_3_clique,
                checksums: [
                    8_459_319_956_822_578_164,
                    6_307_119_068_148_425_140,
                    12_399_277_720_126_721_092,
                ],
            },
        ]
    }

    fn backing_capacities(sim: &Simulation) -> (usize, usize, usize) {
        (
            sim.occupancy.cells.capacity(),
            sim.placements.capacity(),
            sim.attack_grid.cells.capacity(),
        )
    }

    #[test]
    fn hot_path_vec_capacity_growth_is_bounded() {
        let def = GameDefinition::king_6_clique();
        let mut sim = Simulation::new(&def, VisitOrder::default());
        let mut capacity_events = 0usize;

        for _ in 0..100_000 {
            let before = backing_capacities(&sim);
            assert!(sim.step_turn(&def));
            let after = backing_capacities(&sim);
            if after != before {
                capacity_events += 1;
            }
        }
        eprintln!("backing capacity changes during first 100k turns: {capacity_events}");

        let caps = backing_capacities(&sim);
        for _ in 0..5_000 {
            let before = backing_capacities(&sim);
            assert!(sim.step_turn(&def));
            assert_eq!(backing_capacities(&sim), before);
        }

        sim.reset(&def);
        for _ in 0..1_000 {
            let before = backing_capacities(&sim);
            assert!(sim.step_turn(&def));
            assert_eq!(backing_capacities(&sim), before);
        }

        assert_eq!(backing_capacities(&sim), caps);
    }

    #[test]
    fn attack_grid_growth_preserves_marks() {
        let def = GameDefinition::knight_2_pairwise();
        let mut grid = AttackGrid::new(CellWidth::U8);
        // Mark a cell, force several growths, and confirm the bit survives the re-layout.
        assert!(grid.record(3, -5, 1, 2, &[(0, 0)]));
        assert_eq!(grid.at(3, -5), 1);
        let _ = &def;
        assert!(grid.record(200, -180, 0b10, 2, &[(0, 0)]));
        assert_eq!(grid.at(3, -5), 1, "earlier mark must survive growth");
        assert_eq!(grid.at(200, -180), 0b10);
        assert_eq!(grid.at(199, -179), 0, "unmarked cell stays empty");
        grid.clear();
        assert_eq!(grid.at(3, -5), 0, "clear wipes all marks");
        assert_eq!(grid.at(200, -180), 0);
    }

    #[test]
    fn cell_width_matches_highest_respected_bit() {
        assert_eq!(cell_width_for(&[]), CellWidth::U8);
        assert_eq!(cell_width_for(&[0]), CellWidth::U8);
        assert_eq!(cell_width_for(&[0b1, 0b1000_0000]), CellWidth::U8);
        assert_eq!(cell_width_for(&[1 << 8]), CellWidth::U16);
        assert_eq!(cell_width_for(&[1 << 15]), CellWidth::U16);
        assert_eq!(cell_width_for(&[1 << 16]), CellWidth::U32);
        assert_eq!(cell_width_for(&[1 << 31]), CellWidth::U32);
    }

    #[test]
    fn narrow_cells_ignore_out_of_width_attacker_bits() {
        // U8 cells: a respected bit (< 8) is stored, while a higher non-respected attacker bit
        // truncates to zero on write and never corrupts the respected bits.
        let mut grid = AttackGrid::new(CellWidth::U8);
        grid.record(0, 0, (1 << 2) | (1 << 9), 1, &[(0, 0)]);
        assert_eq!(grid.at(0, 0), 1 << 2, "bit 9 truncated, bit 2 preserved");

        // U16 cells hold bits 0..16; verify a high in-range bit round-trips through growth.
        let mut wide = AttackGrid::new(CellWidth::U16);
        wide.record(120, -90, 1 << 12, 2, &[(0, 0)]);
        wide.record(900, 900, 1 << 3, 2, &[(0, 0)]);
        assert_eq!(wide.at(120, -90), 1 << 12, "wide bit survives growth");
        assert_eq!(wide.at(900, 900), 1 << 3);
    }

    #[test]
    fn tiny_budget_saturates_and_renders_partial_without_crashing() {
        let def = GameDefinition::knight_2_pairwise();
        let mut sim = Simulation::new(&def, VisitOrder::default());
        // A few KiB only admits a handful of placements before the budget is hit.
        sim.set_memory_budget_bytes(8 * 1024);
        let target = 5_000_000u32;

        sim.advance_to_target(&def, target);
        assert!(sim.is_saturated(), "tiny budget must saturate");
        let placed = sim.placements.len();
        assert!(placed > 0, "must keep what it placed before saturating");
        // Overshoot is at most one growth step of each structure, nowhere near the target's needs.
        assert!(
            sim.footprint_bytes() < 1 << 20,
            "footprint stays bounded after saturation: {}",
            sim.footprint_bytes()
        );

        // A saturated sim refuses further work instead of growing (or aborting).
        sim.advance_to_target(&def, target);
        assert_eq!(sim.placements.len(), placed, "saturated sim does not advance");

        // Reset clears saturation; with the real budget it runs normally again.
        sim.reset(&def);
        assert!(!sim.is_saturated());
        assert_eq!(sim.placements.len(), 0, "reset wipes placements");
        sim.set_memory_budget_bytes(MEM_BUDGET_BYTES);
        sim.advance_to_target(&def, 64);
        assert!(
            sim.placements.len() > 0 && !sim.is_saturated(),
            "post-reset advance proceeds under the real budget"
        );
    }

    #[test]
    fn early_red_black_placements() {
        let def = GameDefinition::knight_2_pairwise();
        let mut sim = Simulation::new(&def, VisitOrder::default());
        for _ in 0..6 {
            sim.step_turn(&def);
        }
        assert_eq!(sim.occupancy.get(&0), Some(&0));
        assert_eq!(sim.occupancy.get(&1), Some(&1));
        assert_eq!(sim.occupancy.get(&2), Some(&0));
        assert_eq!(sim.occupancy.get(&3), Some(&1));
    }

    #[test]
    fn forbidden_cells_are_cached_per_piece() {
        let def = GameDefinition::knight_2_pairwise();
        let mut sim = Simulation::new(&def, VisitOrder::default());
        sim.step_turn(&def);
        sim.step_turn(&def);

        let red_placement = sim
            .placements
            .as_slice()
            .iter()
            .rev()
            .find_map(|&(idx, pid)| (pid == 1).then_some(idx))
            .expect("red placement");
        let red_xy = index_to_xy(red_placement);
        for &(dx, dy) in &def.pieces[1].piece.valid_moves {
            let (ax, ay) = (red_xy.0 + dx, red_xy.1 + dy);
            assert!(sim.attack_grid.at(ax, ay) & (1u32 << 1) != 0);
        }

        let black_xy = (0, 0);
        assert!(sim.attack_grid.at(black_xy.0 + 1, black_xy.1 + 2) & (1u32 << 0) != 0);
        assert!(sim.occupancy.contains_index(0));
        assert!(sim.occupancy.contains_index(1));
    }

    #[test]
    fn placement_blocking_attacker_matches_move_pattern() {
        let def = GameDefinition::knight_2_pairwise();
        let mut sim = Simulation::new(&def, VisitOrder::default());
        for turn in 0..32 {
            let scanning = sim.active_turn_order[sim.turn_order_index];
            let skips = sim.scan_skips_on_next_scan(&def);
            for &skip in &skips.forbidden {
                for attacker in sim.respected_forbidden_attackers(scanning, skip) {
                    let from = sim
                        .placement_blocking_attacker(&def, attacker, skip)
                        .expect("attacker in forbidden layer must threaten from a placement");
                    assert!(
                        placement_attacks_index(&def, sim.visit_order, from, attacker, skip),
                        "turn {turn} skip {skip} attacker {attacker} from {from}"
                    );
                }
            }
            assert!(sim.step_turn(&def), "turn {turn}");
        }
    }

    #[test]
    fn red_black_first_sixteen_placements_are_stable() {
        let def = GameDefinition::knight_2_pairwise();
        let sim = run_turns(&def, 16);

        assert_eq!(
            sim.placements,
            vec![
                (0, 0),
                (1, 1),
                (2, 0),
                (3, 1),
                (5, 0),
                (4, 1),
                (9, 0),
                (6, 1),
                (11, 0),
                (10, 1),
                (15, 0),
                (12, 1),
                (20, 0),
                (24, 1),
                (21, 0),
                (25, 1),
            ]
        );
    }

    #[test]
    fn representative_preset_checksums_are_stable() {
        for case in golden_cases() {
            let def = (case.def)();
            for (turns, expected_checksum) in GOLDEN_TURNS.into_iter().zip(case.checksums) {
                let sim = run_turns(&def, turns);
                assert_eq!(
                    placement_checksum(&sim.placements),
                    expected_checksum,
                    "{} after {turns} turns",
                    case.name
                );
            }
        }
    }

    #[test]
    fn representative_preset_placements_remain_legal() {
        for case in golden_cases() {
            let def = (case.def)();
            let sim = run_turns(&def, 10_000);

            assert_valid_placements(&def, &sim.placements);
            for (&cursor, &xy) in sim.cursors.iter().zip(&sim.cursor_positions) {
                assert_eq!(index_to_xy(cursor), xy, "{} cursor {cursor}", case.name);
            }
        }
    }

    /// Survey whether cheaper than `xy_to_index` per attack is plausible (run with `--nocapture`).
    #[test]
    fn attack_indexing_survey() {
        use crate::spiral::{index_to_ring_offset, xy_to_ring_offset};
        use std::collections::HashMap;

        struct Survey {
            placements: u64,
            max_abs_xy: i32,
            same_ring_attacks: u64,
            ring_delta_le1: u64,
            /// Distinct `(placement_index, move_slot)` → spiral index delta.
            index_delta_keys: HashMap<(u32, u8), i32>,
            /// Distinct `(placement_ring, move_slot)` → spiral index delta.
            ring_delta_keys: HashMap<(u32, u8), i32>,
            /// Distinct `(placement_ring, perimeter_offset, move_slot)` → delta.
            ring_offset_delta_keys: HashMap<(u32, u32, u8), i32>,
            index_delta_conflicts: u64,
            ring_delta_conflicts: u64,
            ring_offset_delta_conflicts: u64,
            total_attacks: u64,
        }

        fn survey_preset(name: &str, def: &GameDefinition, turns: usize) -> Survey {
            let mut sim = Simulation::new(def, VisitOrder::default());
            let mut s = Survey {
                placements: 0,
                max_abs_xy: 0,
                same_ring_attacks: 0,
                ring_delta_le1: 0,
                index_delta_keys: HashMap::new(),
                ring_delta_keys: HashMap::new(),
                ring_offset_delta_keys: HashMap::new(),
                index_delta_conflicts: 0,
                ring_delta_conflicts: 0,
                ring_offset_delta_conflicts: 0,
                total_attacks: 0,
            };

            for _ in 0..turns {
                assert!(sim.step_turn(def));
                let (place_index, piece_id) = *sim.placements.last().unwrap();
                let (x, y) = index_to_xy(place_index);
                s.max_abs_xy = s.max_abs_xy.max(x.abs()).max(y.abs());
                let place_ro = index_to_ring_offset(place_index);
                let moves = &def.piece(piece_id).piece.valid_moves;
                s.placements += 1;

                for (slot, &(dx, dy)) in moves.iter().enumerate() {
                    s.total_attacks += 1;
                    let attacked = xy_to_index(x + dx, y + dy);
                    let delta = attacked as i64 - place_index as i64;
                    let attack_ro = xy_to_ring_offset(x + dx, y + dy);
                    if attack_ro.ring == place_ro.ring {
                        s.same_ring_attacks += 1;
                    }
                    if attack_ro.ring.abs_diff(place_ro.ring) <= 1 {
                        s.ring_delta_le1 += 1;
                    }
                    let d = delta as i32;
                    match s.index_delta_keys.get(&(place_index, slot as u8)) {
                        Some(prev) if *prev != d => s.index_delta_conflicts += 1,
                        None => {
                            s.index_delta_keys.insert((place_index, slot as u8), d);
                        }
                        _ => {}
                    }
                    match s.ring_delta_keys.get(&(place_ro.ring, slot as u8)) {
                        Some(prev) if *prev != d => s.ring_delta_conflicts += 1,
                        None => {
                            s.ring_delta_keys.insert((place_ro.ring, slot as u8), d);
                        }
                        _ => {}
                    }
                    match s.ring_offset_delta_keys.get(&(
                        place_ro.ring,
                        place_ro.offset,
                        slot as u8,
                    )) {
                        Some(prev) if *prev != d => s.ring_offset_delta_conflicts += 1,
                        None => {
                            s.ring_offset_delta_keys
                                .insert((place_ro.ring, place_ro.offset, slot as u8), d);
                        }
                        _ => {}
                    }
                }
            }
            eprintln!("\n=== attack indexing survey: {name} ({turns} placements) ===");
            eprintln!(
                "  max |x|,|y| at placement: {}; total attacks: {}",
                s.max_abs_xy, s.total_attacks
            );
            let attacks = s.total_attacks;
            eprintln!(
                "  attack ring == placement ring: {:.1}%",
                100.0 * s.same_ring_attacks as f64 / attacks as f64
            );
            eprintln!(
                "  |Δring| <= 1: {:.1}%",
                100.0 * s.ring_delta_le1 as f64 / attacks as f64
            );
            eprintln!(
                "  unique (placement_index, move) → index Δ: {}",
                s.index_delta_keys.len()
            );
            eprintln!(
                "  unique (placement_ring, move) → index Δ: {}",
                s.ring_delta_keys.len()
            );
            eprintln!(
                "  unique (ring, perimeter_offset, move) → index Δ: {} (conflicts {})",
                s.ring_offset_delta_keys.len(),
                s.ring_offset_delta_conflicts
            );
            eprintln!(
                "  index-key conflicts: {}; ring-key conflicts: {}",
                s.index_delta_conflicts, s.ring_delta_conflicts
            );
            s
        }

        let king = GameDefinition::king_6_clique();
        let chimera = GameDefinition::chimera_3_clique();
        survey_preset(
            "knight_2_pairwise",
            &GameDefinition::knight_2_pairwise(),
            100_000,
        );
        survey_preset("king_6_clique", &king, 100_000);
        survey_preset("chimera_3_clique", &chimera, 100_000);

        // Replay 100k knight placements: precomputed index→attacks table vs live xy_to_index.
        use crate::model::PieceDef;
        use std::hint::black_box;
        use std::time::Instant;

        let moves: &[(i32, i32)] = &PieceDef::knight().valid_moves;
        let def = GameDefinition::knight_2_pairwise();
        let mut sim = Simulation::new(&def, VisitOrder::default());
        let mut placements_xy: Vec<(u32, (i32, i32))> = Vec::with_capacity(100_000);
        for _ in 0..100_000 {
            assert!(sim.step_turn(&def));
            let (index, _) = *sim.placements.last().unwrap();
            placements_xy.push((index, index_to_xy(index)));
        }
        let max_index = placements_xy.iter().map(|(i, _)| *i).max().unwrap();
        eprintln!(
            "\n=== attack table vs xy_to_index (knight, 100k placements, max_index={max_index}) ==="
        );

        let mut table = vec![[0u32; 8]; max_index as usize + 1];
        let t0 = Instant::now();
        for index in 0..=max_index {
            let (x, y) = index_to_xy(index);
            for (i, &(dx, dy)) in moves.iter().enumerate() {
                table[index as usize][i] = xy_to_index(x + dx, y + dy);
            }
        }
        let build_ms = t0.elapsed().as_secs_f64() * 1e3;
        eprintln!("  build table [0..={max_index}]: {build_ms:.1} ms");

        let t1 = Instant::now();
        let mut acc = 0u64;
        for &(index, _) in &placements_xy {
            for &a in &table[index as usize] {
                acc = acc.wrapping_add(a as u64);
            }
        }
        black_box(acc);
        let lut_ms = t1.elapsed().as_secs_f64() * 1e3;

        let t2 = Instant::now();
        let mut acc2 = 0u64;
        for &(_, (x, y)) in &placements_xy {
            for &(dx, dy) in moves {
                acc2 = acc2.wrapping_add(xy_to_index(x + dx, y + dy) as u64);
            }
        }
        black_box(acc2);
        let live_ms = t2.elapsed().as_secs_f64() * 1e3;

        eprintln!(
            "  replay 800k attacks — LUT: {lut_ms:.1} ms, live xy_to_index: {live_ms:.1} ms (build amortized +{:.1} ms/placement if spread over 100k)",
            build_ms / 100_000.0
        );
    }
}
