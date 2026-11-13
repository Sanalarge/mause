use crate::config::Side;
use std::collections::HashMap;

#[derive(Clone)]
pub struct Placement {
    pub key: String,
    pub parent: String,
    pub side: Side,
    pub cell: (i32, i32),
}

pub fn step(cell: (i32, i32), side: Side) -> (i32, i32) {
    match side {
        Side::Left => (cell.0 - 1, cell.1),
        Side::Right => (cell.0 + 1, cell.1),
        Side::Top => (cell.0, cell.1 - 1),
        Side::Bottom => (cell.0, cell.1 + 1),
    }
}

pub fn resolve(mut pending: Vec<Placement>) -> Vec<Placement> {
    let keys: std::collections::HashSet<_> = pending.iter().map(|p| p.key.clone()).collect();
    for placement in &mut pending {
        if !keys.contains(&placement.parent) || placement.parent == placement.key {
            placement.parent.clear();
        }
    }
    let mut cells = HashMap::from([(String::new(), (0, 0))]);
    let mut occupied = HashMap::from([((0, 0), String::new())]);
    let mut resolved = Vec::new();
    while !pending.is_empty() {
        let index = pending.iter().position(|p| cells.contains_key(&p.parent));
        let index = index.unwrap_or_else(|| {
            pending[0].parent.clear();
            0
        });
        let mut placement = pending.remove(index);
        let mut at = cells[&placement.parent];
        loop {
            let next = step(at, placement.side);
            match occupied.get(&next) {
                Some(key) if key.is_empty() => {
                    placement.side = placement.side.opposite();
                }
                Some(key) => {
                    placement.parent = key.clone();
                    at = next;
                }
                None => {
                    placement.cell = next;
                    occupied.insert(next, placement.key.clone());
                    cells.insert(placement.key.clone(), next);
                    resolved.push(placement);
                    break;
                }
            }
        }
    }
    resolved
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(key: &str, parent: &str, side: Side) -> Placement {
        Placement {
            key: key.into(),
            parent: parent.into(),
            side,
            cell: (0, 0),
        }
    }

    #[test]
    fn any_number_of_colliding_devices_extend_the_same_edge_without_overlap() {
        for side in Side::ALL {
            let entries: Vec<_> = (0..12).map(|i| node(&format!("pc{i}"), "", side)).collect();
            let resolved = resolve(entries);
            let mut cell = (0, 0);
            for (i, placement) in resolved.iter().enumerate() {
                cell = step(cell, side);
                assert_eq!(placement.cell, cell);
                assert_eq!(
                    placement.parent,
                    if i == 0 {
                        String::new()
                    } else {
                        format!("pc{}", i - 1)
                    }
                );
            }
        }
    }

    #[test]
    fn nested_positions_and_geometric_collisions_are_resolved_without_hiding_devices() {
        let placements = resolve(vec![
            node("child", "A", Side::Right),
            node("A", "", Side::Right),
            node("B", "", Side::Right),
            node("top", "", Side::Top),
            node("corner", "top", Side::Right),
            node("overlap", "A", Side::Top),
        ]);
        let unique: std::collections::HashSet<_> = placements.iter().map(|p| p.cell).collect();
        assert_eq!(unique.len(), 6);
        let child = placements.iter().find(|p| p.key == "child").unwrap();
        assert_eq!(child.parent, "A");
        assert_eq!(child.cell, (2, 0));
        let b = placements.iter().find(|p| p.key == "B").unwrap();
        assert_eq!(b.parent, "child");
        assert_eq!(b.cell, (3, 0));
    }

    #[test]
    fn missing_parents_cycles_and_links_back_into_the_local_pc_do_not_lose_devices() {
        let placements = resolve(vec![
            node("A", "missing", Side::Right),
            node("B", "A", Side::Left),
            node("C", "D", Side::Top),
            node("D", "C", Side::Bottom),
        ]);
        assert_eq!(placements.len(), 4);
        assert!(placements.iter().all(|p| p.cell != (0, 0)));
        let unique: std::collections::HashSet<_> = placements.iter().map(|p| p.cell).collect();
        assert_eq!(unique.len(), 4);
    }
}
