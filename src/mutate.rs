use rand::Rng;

use crate::model::{Army, ArmyId};

pub fn toggle_random_attack_square(moves: &mut Vec<(i32, i32)>, rng: &mut impl Rng) {
    let r = move_extent(moves).max(2);
    for _ in 0..64 {
        let x = rng.random_range(-r..=r);
        let y = rng.random_range(-r..=r);
        if x == 0 && y == 0 {
            continue;
        }
        toggle_square(moves, x, y);
        return;
    }
}

pub fn toggle_square(moves: &mut Vec<(i32, i32)>, x: i32, y: i32) {
    if x == 0 && y == 0 {
        return;
    }
    if let Some(idx) = moves.iter().position(|&m| m == (x, y)) {
        if moves.len() > 1 {
            moves.remove(idx);
        }
    } else {
        moves.push((x, y));
    }
    normalize_moves(moves);
}

/// Shift attack cells on a fixed `(2r+1)²` grid; each row/column wraps like a ring buffer.
/// Pass `dx` or `dy` as `±1` (the other `0`). Preserves attack count and grid radius.
pub fn shift_attacks(
    moves: &mut Vec<(i32, i32)>,
    dx: i32,
    dy: i32,
    fixed_radius: Option<i32>,
) {
    if moves.is_empty() || (dx == 0 && dy == 0) {
        return;
    }
    let r = fixed_radius.unwrap_or_else(|| move_extent(moves)).max(1);
    let size = (2 * r + 1) as usize;
    let origin = r as usize;

    let mut grid = vec![vec![false; size]; size];
    for &(x, y) in moves.iter() {
        if x.abs().max(y.abs()) > r {
            continue;
        }
        grid[(x + r) as usize][(y + r) as usize] = true;
    }
    grid[origin][origin] = false;

    let mut work = grid;
    if dy != 0 {
        let mut next = vec![vec![false; size]; size];
        for xi in 0..size {
            let col: Vec<bool> = (0..size).map(|yi| work[xi][yi]).collect();
            let shifted = shift_line(&col, dy);
            for yi in 0..size {
                next[xi][yi] = shifted[yi];
            }
        }
        work = next;
    }
    if dx != 0 {
        let mut next = vec![vec![false; size]; size];
        for yi in 0..size {
            let row: Vec<bool> = (0..size).map(|xi| work[xi][yi]).collect();
            let shifted = shift_line(&row, dx);
            for xi in 0..size {
                next[xi][yi] = shifted[xi];
            }
        }
        work = next;
    }

    work[origin][origin] = false;
    *moves = moves_from_grid(&work, r);
    normalize_moves(moves);
}

pub fn attack_extent(moves: &[(i32, i32)]) -> i32 {
    move_extent(moves).max(1)
}

pub fn shared_attack_extent_for_armies(armies: &[Army], ids: &[usize]) -> i32 {
    ids.iter()
        .map(|&i| attack_extent(&armies[i].piece.valid_moves))
        .max()
        .unwrap_or(1)
}

/// `delta > 0`: index `i` takes from `i - 1`; `delta < 0`: index `i` takes from `i + 1`.
fn shift_line(old: &[bool], delta: i32) -> Vec<bool> {
    let n = old.len();
    if delta == 0 {
        return old.to_vec();
    }
    let step = if delta > 0 { 1 } else { n - 1 };
    (0..n).map(|i| old[(i + n - step) % n]).collect()
}

fn moves_from_grid(grid: &[Vec<bool>], r: i32) -> Vec<(i32, i32)> {
    let size = grid.len();
    let mut moves = Vec::new();
    for yi in 0..size {
        for xi in 0..size {
            if grid[xi][yi] {
                let x = xi as i32 - r;
                let y = yi as i32 - r;
                if x != 0 || y != 0 {
                    moves.push((x, y));
                }
            }
        }
    }
    moves
}

pub fn reflect_across_x_axis(moves: &mut Vec<(i32, i32)>) {
    map_moves(moves, |(x, y)| (x, -y));
}

pub fn reflect_across_y_axis(moves: &mut Vec<(i32, i32)>) {
    map_moves(moves, |(x, y)| (-x, y));
}

pub fn rotate_cw(moves: &mut Vec<(i32, i32)>) {
    map_moves(moves, |(x, y)| (y, -x));
}

pub fn rotate_ccw(moves: &mut Vec<(i32, i32)>) {
    map_moves(moves, |(x, y)| (-y, x));
}

pub fn toggle_random_blocked_by(
    army: &mut Army,
    army_idx: ArmyId,
    army_count: usize,
    rng: &mut impl Rng,
) {
    if army_count <= 1 {
        return;
    }
    for _ in 0..32 {
        let other = rng.random_range(0..army_count);
        if other == army_idx {
            continue;
        }
        if let Some(pos) = army.blocked_by.iter().position(|&id| id == other) {
            army.blocked_by.remove(pos);
        } else {
            army.blocked_by.push(other);
        }
        return;
    }
}

fn map_moves(moves: &mut Vec<(i32, i32)>, f: impl Fn((i32, i32)) -> (i32, i32)) {
    if moves.is_empty() {
        return;
    }
    *moves = moves.iter().map(|&m| f(m)).collect();
    moves.retain(|&(x, y)| x != 0 || y != 0);
    normalize_moves(moves);
}

fn move_extent(moves: &[(i32, i32)]) -> i32 {
    moves
        .iter()
        .map(|&(x, y)| x.abs().max(y.abs()))
        .max()
        .unwrap_or(1)
}

fn normalize_moves(moves: &mut Vec<(i32, i32)>) {
    moves.sort_by_key(|&(x, y)| (x, y));
    moves.dedup();
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;
    use rand::rngs::StdRng;

    #[test]
    fn shift_wrap_preserves_count_and_radius() {
        let mut moves = vec![(1, 0), (0, 2)];
        let n = moves.len();
        let r = move_extent(&moves);
        shift_attacks(&mut moves, 0, 1, None);
        assert_eq!(moves.len(), n);
        assert_eq!(move_extent(&moves), r);
        assert!(!moves.iter().any(|&m| m == (0, 0)));
    }

    #[test]
    fn toggle_never_leaves_empty_pattern() {
        let mut moves = vec![(1, 0)];
        let mut rng = StdRng::seed_from_u64(1);
        toggle_square(&mut moves, 1, 0);
        assert_eq!(moves, vec![(1, 0)]);
        toggle_random_attack_square(&mut moves, &mut rng);
        assert!(!moves.is_empty());
    }
}
