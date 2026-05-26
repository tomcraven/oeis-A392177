//! Square spiral numbering matching Numberphile / OEIS A316667 (1-based in OEIS, 0-based here).
//! Center is 0 at (0, 0); index 1 is east; spiral runs counterclockwise.

/// Spiral index → grid coordinates (x east, y north).
#[cfg_attr(not(test), allow(dead_code))]
pub fn index_to_xy(index: u32) -> (i32, i32) {
    if index == 0 {
        return (0, 0);
    }
    let ring = (index.isqrt() + 1) / 2;
    let ring_i = ring as i32;
    let side_len = 2 * ring;
    let inner_side = 2 * ring - 1;
    let start = inner_side * inner_side;
    let offset = index - start;

    if offset < side_len {
        return (ring_i, -ring_i + 1 + offset as i32);
    }
    if offset < 2 * side_len {
        return (ring_i - 1 - (offset - side_len) as i32, ring_i);
    }
    if offset < 3 * side_len {
        return (-ring_i, ring_i - 1 - (offset - 2 * side_len) as i32);
    }
    (-ring_i + 1 + (offset - 3 * side_len) as i32, -ring_i)
}

/// Grid coordinates → spiral index.
pub fn xy_to_index(x: i32, y: i32) -> u32 {
    if x == 0 && y == 0 {
        return 0;
    }
    let ring = x.abs().max(y.abs());
    let ring_u = ring as u32;
    let side_len = 2 * ring_u;
    let inner_side = 2 * ring_u - 1;
    let start = inner_side * inner_side;

    let offset = if x == ring && y >= -ring + 1 {
        (y + ring - 1) as u32
    } else if y == ring {
        side_len + (ring - 1 - x) as u32
    } else if x == -ring {
        2 * side_len + (ring - 1 - y) as u32
    } else {
        3 * side_len + (x + ring - 1) as u32
    };

    start + offset
}

pub fn spiral_step((x, y): (i32, i32)) -> (i32, i32) {
    if x == 0 && y == 0 {
        return (1, 0);
    }
    let w = x.abs().max(y.abs());
    if y == -w {
        (x + 1, y)
    } else if x == -w {
        (x, y - 1)
    } else if y == w {
        (x - 1, y)
    } else {
        (x, y + 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_near_origin() {
        for index in 0..20_000 {
            let (x, y) = index_to_xy(index);
            assert_eq!(xy_to_index(x, y), index, "index {index} -> ({x},{y})");
        }
    }

    #[test]
    fn step_matches_index_sequence() {
        let mut xy = (0, 0);
        for index in 1..20_000 {
            xy = spiral_step(xy);
            assert_eq!(xy, index_to_xy(index));
        }
    }

    #[test]
    fn oesis_diagram_1_based_mapping() {
        assert_eq!(index_to_xy(0), (0, 0));
        assert_eq!(index_to_xy(1), (1, 0));
        assert_eq!(index_to_xy(2), (1, 1));
        assert_eq!(index_to_xy(3), (0, 1));
        assert_eq!(index_to_xy(4), (-1, 1));
        assert_eq!(index_to_xy(5), (-1, 0));
    }
}
