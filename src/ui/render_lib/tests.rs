use super::*;

fn zap(last_segment_len_prop: f32) -> Zappy {
    Zappy {
        hops: 10,
        variance: 2.5,
        speed: 0.1,
        flow_irregular: 0.2,
        readability_offset: 0.2,
        last_segment_len_prop,
        seed: 0.0,
        ticks: 12.5,
    }
}
use crate::math::HexDir;

#[test]
fn px_round_trips() {
    let off = Vec2::new(213.0, 120.0);
    for q in -4..=4 {
        for r in -4..=4 {
            let c = HexCoord(q, r);
            let px = coord_to_px(c, 18.0, off);
            assert_eq!(px_to_coord(px, 18.0, off), c);
            assert_eq!(px_to_coord(px + Vec2::new(3.0, -2.5), 18.0, off), c);
        }
    }
}

#[test]
fn range_around_is_a_hexagon() {
    let pts = range_around(HexCoord(2, -1), 3);
    assert_eq!(pts.len(), 37);
    assert!(pts.contains(&HexCoord(5, -1)));
    assert!(!pts.contains(&HexCoord(5, 0)));
}

#[test]
fn dup_indices() {
    let d = find_dup_indices([1, 2, 3, 1, 4]);
    assert_eq!(d, HashSet::from([0, 3]));
}

#[test]
fn zappy_keeps_the_ends() {
    let pts = [
        Vec2::new(0.0, 0.0),
        Vec2::new(30.0, 0.0),
        Vec2::new(45.0, 26.0),
    ];
    let z = make_zappy(&pts, None, &zap(1.0));
    assert_eq!(z[0], pts[0]);
    assert_eq!(*z.last().unwrap(), pts[2]);
    assert_eq!(z.len(), 1 + 2 * 11);
    // Stopping short of the last point (the line being drawn).
    let z = make_zappy(&pts, None, &zap(0.8));
    assert_eq!(z.len(), 1 + 11 + 8);
}

#[test]
fn line_and_spot_meshes() {
    let mut m = Mesh::new();
    draw_spot(&mut m, Vec2::new(5.0, 5.0), 2.0, 0xFFFFFFFF);
    assert_eq!(m.vertex_count(), 6 * 3);
    let mut m = Mesh::new();
    let line = Line {
        width: 5.0,
        tail: 0xFF000000,
        head: 0xFFFFFFFF,
        gradient: true,
    };
    draw_line_seq(&mut m, &[Vec2::ZERO, Vec2::new(10.0, 0.0)], line);
    // One segment (4 triangles) and two caps of 10 triangles.
    assert_eq!(m.vertex_count(), (4 + 20) * 3);
}

#[test]
fn centered_pattern_is_centered() {
    let p = HexPattern::from_angles("qaq", HexDir::NorthEast).unwrap();
    let (scale, pts) = get_centered_pattern(&p, 128.0, 128.0, 16.0);
    assert!(scale > 0.0 && scale <= 16.0);
    let c = find_center(&pts);
    assert!(c.x.abs() < 1e-3 && c.y.abs() < 1e-3);
}
