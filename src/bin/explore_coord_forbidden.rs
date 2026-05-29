//! Investigation harness (perf-iteration "next target": call `xy_to_index` less often).
//!
//! In the sim hot path, `xy_to_index` is called ONLY in `record_forbidden` (once per
//! move per placement); the scan loop walks `(x, y)` incrementally with `spiral_step`.
//! This binary measures whether a **2D coordinate forbidden grid** (per-cell attacker
//! bitmask, marked via `row*stride+col` with no `xy_to_index`) beats the production
//! per-attacker **spiral-index bitset** (which needs one `xy_to_index` per attacked cell).
//!
//! Both representations run behind the SAME scan/occupancy code so the only variable is
//! forbidden storage. Checksums must match the golden `time_sim` values for both.
//!
//! Run: `cargo run --release --features bevy/dynamic_linking --bin explore_coord_forbidden`
//! Env: `EXPLORE_ITERS` (default 25), `EXPLORE_WARMUP` (default 3), `EXPLORE_TURNS` (100k).

use std::hint::black_box;
use std::time::{Duration, Instant};

use red_black_knights::model::{GameDefinition, PieceId};
use red_black_knights::spiral::{spiral_step, xy_to_index};

const EMPTY: PieceId = usize::MAX;
const DEFAULT_TURNS: usize = 100_000;
const DEFAULT_WARMUP: usize = 3;
const DEFAULT_ITERS: usize = 25;

trait Forbidden {
    fn setup(def: &GameDefinition) -> Self;
    /// Mark every cell attacked by `attacker` placed at `(px, py)`.
    fn record(&mut self, def: &GameDefinition, px: i32, py: i32, attacker: PieceId);
    /// Is `(cx, cy)` / spiral index `ci` forbidden for defender `pid`?
    fn forbidden_at(&self, pid: PieceId, cx: i32, cy: i32, ci: u32) -> bool;
}

/// Production-equivalent: per-attacker spiral-index bitset; one `xy_to_index` per attacked cell.
struct SpiralBitset {
    layers: Vec<Vec<u64>>,
    respected: Vec<Vec<PieceId>>,
}

impl Forbidden for SpiralBitset {
    fn setup(def: &GameDefinition) -> Self {
        let n = def.pieces.len();
        let mut respected = vec![Vec::new(); n];
        for d in 0..n {
            for a in 0..n {
                if def.piece(d).blocked_by.contains(&a) {
                    respected[d].push(a);
                }
            }
        }
        Self {
            layers: vec![Vec::new(); n],
            respected,
        }
    }

    #[inline]
    fn record(&mut self, def: &GameDefinition, px: i32, py: i32, attacker: PieceId) {
        let words = &mut self.layers[attacker];
        for &(dx, dy) in &def.piece(attacker).piece.valid_moves {
            let attacked = xy_to_index(px + dx, py + dy);
            let wi = attacked as usize >> 6;
            let bit = 1u64 << (attacked & 63);
            if wi >= words.len() {
                words.resize(wi + 1, 0);
            }
            words[wi] |= bit;
        }
    }

    #[inline]
    fn forbidden_at(&self, pid: PieceId, _cx: i32, _cy: i32, ci: u32) -> bool {
        let wi = ci as usize >> 6;
        let bit = 1u64 << (ci & 63);
        for &a in &self.respected[pid] {
            if self.layers[a].get(wi).copied().unwrap_or(0) & bit != 0 {
                return true;
            }
        }
        false
    }
}

/// Experimental: single 2D coordinate grid of attacker bitmasks; NO `xy_to_index` to mark.
struct CoordGrid {
    half: i32,
    stride: usize,
    /// `mask[(y+half)*stride + (x+half)]` = bitmask of attackers hitting `(x, y)`.
    mask: Vec<u32>,
    respected_mask: Vec<u32>,
    max_radius: Vec<i32>,
}

impl CoordGrid {
    #[inline]
    fn idx(&self, x: i32, y: i32) -> usize {
        (y + self.half) as usize * self.stride + (x + self.half) as usize
    }

    #[cold]
    fn grow_to(&mut self, need: i32) {
        let new_half = (self.half * 2).max(need + 1);
        let new_stride = (2 * new_half + 1) as usize;
        let mut new_mask = vec![0u32; new_stride * new_stride];
        for y in -self.half..=self.half {
            let src_row = (y + self.half) as usize * self.stride;
            let dst_row = (y + new_half) as usize * new_stride;
            for x in -self.half..=self.half {
                let v = self.mask[src_row + (x + self.half) as usize];
                if v != 0 {
                    new_mask[dst_row + (x + new_half) as usize] = v;
                }
            }
        }
        self.half = new_half;
        self.stride = new_stride;
        self.mask = new_mask;
    }

    #[inline]
    fn at(&self, x: i32, y: i32) -> u32 {
        if x > self.half || x < -self.half || y > self.half || y < -self.half {
            return 0;
        }
        self.mask[self.idx(x, y)]
    }
}

impl Forbidden for CoordGrid {
    fn setup(def: &GameDefinition) -> Self {
        let n = def.pieces.len();
        assert!(n <= 32, "coord grid attacker bitmask is u32");
        let mut respected_mask = vec![0u32; n];
        for d in 0..n {
            for a in 0..n {
                if def.piece(d).blocked_by.contains(&a) {
                    respected_mask[d] |= 1 << a;
                }
            }
        }
        let max_radius = (0..n)
            .map(|a| {
                def.piece(a)
                    .piece
                    .valid_moves
                    .iter()
                    .map(|&(dx, dy)| dx.abs().max(dy.abs()))
                    .max()
                    .unwrap_or(0)
            })
            .collect();
        let half = 8i32;
        let stride = (2 * half + 1) as usize;
        Self {
            half,
            stride,
            mask: vec![0u32; stride * stride],
            respected_mask,
            max_radius,
        }
    }

    #[inline]
    fn record(&mut self, def: &GameDefinition, px: i32, py: i32, attacker: PieceId) {
        // One abs/max per placement (not per move): pre-grow to cover all attacked cells,
        // then mark with a plain row*stride+col — no `xy_to_index`.
        let reach = px.abs().max(py.abs()) + self.max_radius[attacker];
        if reach > self.half {
            self.grow_to(reach);
        }
        let bit = 1u32 << attacker;
        let half = self.half;
        let stride = self.stride;
        let base = (py + half) as usize * stride + (px + half) as usize;
        for &(dx, dy) in &def.piece(attacker).piece.valid_moves {
            let i = (base as isize + dy as isize * stride as isize + dx as isize) as usize;
            self.mask[i] |= bit;
        }
    }

    #[inline]
    fn forbidden_at(&self, pid: PieceId, cx: i32, cy: i32, _ci: u32) -> bool {
        self.at(cx, cy) & self.respected_mask[pid] != 0
    }
}

fn run<F: Forbidden>(def: &GameDefinition, turns: usize) -> (usize, u64) {
    let order: Vec<PieceId> = def.active_turn_order().iter().collect();
    let n = order.len();
    if n == 0 {
        return (0, 0);
    }
    let piece_count = def.pieces.len();
    let mut forbidden = F::setup(def);
    let mut occ: Vec<PieceId> = Vec::new();
    let mut cursors = vec![0u32; piece_count];
    let mut cursor_xy = vec![(0i32, 0i32); piece_count];
    let mut placements: Vec<(u32, PieceId)> = Vec::with_capacity(turns);
    let mut toi = 0usize;

    for _ in 0..turns {
        let pid = order[toi];
        toi += 1;
        if toi == n {
            toi = 0;
        }

        let mut ci = cursors[pid];
        let mut xy = cursor_xy[pid];
        loop {
            let occupied = occ.get(ci as usize).copied().unwrap_or(EMPTY) != EMPTY;
            let forb = forbidden.forbidden_at(pid, xy.0, xy.1, ci);
            if !occupied && !forb {
                if ci as usize >= occ.len() {
                    occ.resize(ci as usize + 1, EMPTY);
                }
                occ[ci as usize] = pid;
                forbidden.record(def, xy.0, xy.1, pid);
                placements.push((ci, pid));
                cursors[pid] = ci.saturating_add(1);
                cursor_xy[pid] = spiral_step(xy);
                break;
            }
            let next = ci.wrapping_add(1);
            if next == 0 {
                cursors[pid] = ci;
                cursor_xy[pid] = xy;
                return (placements.len(), placement_checksum(&placements));
            }
            ci = next;
            xy = spiral_step(xy);
        }
    }

    (placements.len(), placement_checksum(&placements))
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

fn time_repr<F: Forbidden>(def: &GameDefinition, turns: usize, warmup: usize, iters: usize) -> (f64, u64) {
    for _ in 0..warmup {
        black_box(run::<F>(def, turns));
    }
    let mut samples = Vec::with_capacity(iters);
    let mut checksum = 0u64;
    for _ in 0..iters {
        let start = Instant::now();
        let (_, c) = run::<F>(def, turns);
        samples.push(start.elapsed());
        checksum = c;
        black_box(c);
    }
    (median_ms(&samples), checksum)
}

fn median_ms(samples: &[Duration]) -> f64 {
    let mut ms: Vec<f64> = samples.iter().map(|d| d.as_secs_f64() * 1e3).collect();
    ms.sort_by(|a, b| a.partial_cmp(b).unwrap());
    if ms.len() % 2 == 0 {
        (ms[ms.len() / 2 - 1] + ms[ms.len() / 2]) / 2.0
    } else {
        ms[ms.len() / 2]
    }
}

fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|s| s.parse().ok())
        .filter(|&n| n > 0)
        .unwrap_or(default)
}

fn main() {
    let turns = env_usize("EXPLORE_TURNS", DEFAULT_TURNS);
    let warmup = env_usize("EXPLORE_WARMUP", DEFAULT_WARMUP);
    let iters = env_usize("EXPLORE_ITERS", DEFAULT_ITERS);

    let cases: [(&str, fn() -> GameDefinition); 5] = [
        ("knight_2_pairwise", GameDefinition::knight_2_pairwise),
        ("knight_3_clique", GameDefinition::knight_3_clique),
        ("leaper_4_mixed_clique", GameDefinition::leaper_4_mixed_clique),
        ("king_6_clique", GameDefinition::king_6_clique),
        ("chimera_3_clique", GameDefinition::chimera_3_clique),
    ];

    println!(
        "case\tspiral_ms\tcoord_ms\tdelta_pct\tchecksums_match\tchecksum"
    );

    for (name, def_fn) in cases {
        let def = def_fn();
        // Alternate to share thermal conditions.
        let (spiral_ms, spiral_ck) = time_repr::<SpiralBitset>(&def, turns, warmup, iters);
        let (coord_ms, coord_ck) = time_repr::<CoordGrid>(&def, turns, warmup, iters);
        let delta = (coord_ms - spiral_ms) / spiral_ms * 100.0;
        println!(
            "{name}\t{spiral_ms:.3}\t{coord_ms:.3}\t{delta:+.1}%\t{}\t{}",
            spiral_ck == coord_ck,
            coord_ck
        );
    }
}
