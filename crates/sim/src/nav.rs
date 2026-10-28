use std::cmp::Ordering;
use std::collections::BinaryHeap;

use asset::map::NavGrid;
use glam::Vec2;

const SEARCH_RINGS: i32 = 40;
const DIAGONAL: f32 = std::f32::consts::SQRT_2;

pub fn walkable(grid: &NavGrid, point: Vec2) -> bool {
    grid.is_walkable(point.x, point.y)
}

pub fn clear_line(grid: &NavGrid, from: Vec2, to: Vec2) -> bool {
    let origin = Vec2::from(grid.min);
    let start = (from - origin) / grid.cell_size;
    let end = (to - origin) / grid.cell_size;
    let delta = end - start;
    let mut cell = start.floor().as_ivec2();
    let last = end.floor().as_ivec2();
    let step = delta.signum().as_ivec2();
    let edge = |position: f32, index: i32, step: i32, delta: f32| {
        if delta == 0.0 {
            f32::INFINITY
        } else {
            (index as f32 + if step > 0 { 1.0 } else { 0.0 } - position) / delta
        }
    };
    let mut crossing = Vec2::new(
        edge(start.x, cell.x, step.x, delta.x),
        edge(start.y, cell.y, step.y, delta.y),
    );
    let stride = Vec2::new(1.0 / delta.x.abs(), 1.0 / delta.y.abs());
    let open = |cell: glam::IVec2| {
        cell.x >= 0
            && cell.y >= 0
            && (cell.x as u32) < grid.width
            && (cell.y as u32) < grid.height
            && grid.flags[(cell.y as u32 * grid.width + cell.x as u32) as usize] & NavGrid::WALL
                == 0
    };
    let crossings = (last - cell).abs();
    for _ in 0..=crossings.x + crossings.y {
        if !open(cell) {
            return false;
        }
        if cell == last {
            break;
        }
        if crossing.x < crossing.y {
            cell.x += step.x;
            crossing.x += stride.x;
        } else {
            cell.y += step.y;
            crossing.y += stride.y;
        }
    }
    true
}

pub fn last_walkable(grid: &NavGrid, from: Vec2, to: Vec2) -> Vec2 {
    let distance = from.distance(to);
    let steps = (distance / (grid.cell_size * 0.4)).ceil().max(1.0) as u32;
    let mut last = from;
    for step in 1..=steps {
        let point = from.lerp(to, step as f32 / steps as f32);
        if !walkable(grid, point) {
            break;
        }
        last = point;
    }
    last
}

pub fn block(grid: &mut NavGrid, center: Vec2, radius: f32) {
    let size = grid.cell_size;
    let reach = (radius / size).ceil() as i32;
    let Some((u, v)) = grid.cell_at(center.x, center.y) else {
        return;
    };
    for du in -reach..=reach {
        for dv in -reach..=reach {
            let (x, z) = (u as i32 + du, v as i32 + dv);
            if x < 0 || z < 0 || x >= grid.width as i32 || z >= grid.height as i32 {
                continue;
            }
            let cell = Vec2::from_array(grid.cell_center((x as u32, z as u32)));
            let half = Vec2::splat(size * 0.5);
            if center.clamp(cell - half, cell + half).distance(center) <= radius {
                grid.flags[(z as u32 * grid.width + x as u32) as usize] |= NavGrid::WALL;
            }
        }
    }
}

pub fn nearest_walkable(grid: &NavGrid, point: Vec2) -> Vec2 {
    if walkable(grid, point) {
        return point;
    }
    let size = grid.cell_size;
    for ring in 1..=SEARCH_RINGS {
        let mut best: Option<Vec2> = None;
        for dx in -ring..=ring {
            for dz in -ring..=ring {
                if dx.abs() != ring && dz.abs() != ring {
                    continue;
                }
                let candidate = point + Vec2::new(dx as f32, dz as f32) * size;
                if walkable(grid, candidate)
                    && best.is_none_or(|best| candidate.distance(point) < best.distance(point))
                {
                    best = Some(candidate);
                }
            }
        }
        if let Some(best) = best {
            return best;
        }
    }
    point
}

#[derive(PartialEq)]
struct Open {
    cost: f32,
    cell: usize,
}

impl Eq for Open {}

impl Ord for Open {
    fn cmp(&self, other: &Self) -> Ordering {
        other.cost.total_cmp(&self.cost)
    }
}

impl PartialOrd for Open {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

pub fn route(grid: &NavGrid, from: Vec2, to: Vec2) -> Vec<Vec2> {
    let goal = nearest_walkable(grid, to);
    if clear_line(grid, from, goal) {
        return vec![goal];
    }
    let (Some(start), Some(end)) = (grid.cell_at(from.x, from.y), grid.cell_at(goal.x, goal.y))
    else {
        return vec![goal];
    };
    let width = grid.width as usize;
    let index = |(x, z): (u32, u32)| z as usize * width + x as usize;
    let cell = |index: usize| ((index % width) as u32, (index / width) as u32);
    let open_cell = |index: usize| grid.flags[index] & NavGrid::WALL == 0;
    let heuristic = |index: usize| {
        let (x, z) = cell(index);
        let (ex, ez) = end;
        let dx = (x as f32 - ex as f32).abs();
        let dz = (z as f32 - ez as f32).abs();
        dx.max(dz) + (DIAGONAL - 1.0) * dx.min(dz)
    };
    let count = grid.flags.len();
    let mut cost = vec![f32::INFINITY; count];
    let mut parent = vec![usize::MAX; count];
    let mut heap = BinaryHeap::new();
    let start = index(start);
    let end_index = index(end);
    cost[start] = 0.0;
    heap.push(Open {
        cost: heuristic(start),
        cell: start,
    });
    let mut found = false;
    while let Some(Open { cell: current, .. }) = heap.pop() {
        if current == end_index {
            found = true;
            break;
        }
        let (x, z) = cell(current);
        for (dx, dz) in [
            (-1, 0),
            (1, 0),
            (0, -1),
            (0, 1),
            (-1, -1),
            (1, -1),
            (-1, 1),
            (1, 1),
        ] {
            let nx = x as i64 + dx;
            let nz = z as i64 + dz;
            if nx < 0 || nz < 0 || nx >= i64::from(grid.width) || nz >= i64::from(grid.height) {
                continue;
            }
            let next = index((nx as u32, nz as u32));
            if !open_cell(next) {
                continue;
            }
            if dx != 0
                && dz != 0
                && (!open_cell(index((nx as u32, z))) || !open_cell(index((x, nz as u32))))
            {
                continue;
            }
            let step = if dx != 0 && dz != 0 { DIAGONAL } else { 1.0 };
            let candidate = cost[current] + step;
            if candidate < cost[next] {
                cost[next] = candidate;
                parent[next] = current;
                heap.push(Open {
                    cost: candidate + heuristic(next),
                    cell: next,
                });
            }
        }
    }
    if !found {
        return vec![last_walkable(grid, from, goal)];
    }
    let mut cells = vec![end_index];
    while let Some(&last) = cells.last() {
        let previous = parent[last];
        if previous == usize::MAX {
            break;
        }
        cells.push(previous);
    }
    cells.reverse();
    let mut points: Vec<Vec2> = cells
        .into_iter()
        .map(|index| Vec2::from(grid.cell_center(cell(index))))
        .collect();
    if let Some(last) = points.last_mut() {
        *last = goal;
    }
    smooth(grid, from, &points)
}

fn smooth(grid: &NavGrid, from: Vec2, points: &[Vec2]) -> Vec<Vec2> {
    let mut result = Vec::new();
    let mut anchor = from;
    let mut index = 0;
    while index < points.len() {
        let mut furthest = index;
        for candidate in (index..points.len()).rev() {
            if clear_line(grid, anchor, points[candidate]) {
                furthest = candidate;
                break;
            }
        }
        result.push(points[furthest]);
        anchor = points[furthest];
        index = furthest + 1;
    }
    result
}

#[cfg(test)]
mod tests {
    use asset::map::HeightField;

    use super::*;

    fn grid() -> NavGrid {
        let width = 10;
        let height = 10;
        let flags = (0..width * height)
            .map(|index| {
                let (x, z) = (index % width, index / width);
                if x == 5 && z < 8 { NavGrid::WALL } else { 0 }
            })
            .collect();
        NavGrid {
            min: [0.0, 0.0],
            cell_size: 100.0,
            width,
            height,
            flags,
            heights: HeightField::default(),
        }
    }

    #[test]
    fn lines_that_clip_a_wall_corner_are_blocked() {
        let grid = grid();
        assert!(!clear_line(
            &grid,
            Vec2::new(480.0, 815.0),
            Vec2::new(620.0, 795.0)
        ));
        assert!(clear_line(
            &grid,
            Vec2::new(480.0, 815.0),
            Vec2::new(620.0, 805.0)
        ));
    }

    #[test]
    fn walks_straight_when_nothing_is_in_the_way() {
        let grid = grid();
        let path = route(&grid, Vec2::new(50.0, 50.0), Vec2::new(350.0, 750.0));
        assert_eq!(path, vec![Vec2::new(350.0, 750.0)]);
    }

    #[test]
    fn goes_around_walls() {
        let grid = grid();
        let from = Vec2::new(250.0, 150.0);
        let to = Vec2::new(850.0, 150.0);
        let path = route(&grid, from, to);
        assert_eq!(path.last(), Some(&to));
        assert!(path.len() >= 2);
        let mut anchor = from;
        for point in &path {
            assert!(clear_line(&grid, anchor, *point), "{anchor} -> {point}");
            anchor = *point;
        }
        assert!(path.iter().any(|point| point.y >= 800.0));
    }

    #[test]
    fn clicks_inside_walls_move_to_the_closest_open_spot() {
        let grid = grid();
        let target = nearest_walkable(&grid, Vec2::new(540.0, 300.0));
        assert!(walkable(&grid, target));
        assert!(target.distance(Vec2::new(540.0, 300.0)) <= 100.0);
    }
}
