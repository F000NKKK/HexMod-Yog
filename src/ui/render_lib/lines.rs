//! `RenderLib`'s lines: polylines with round joins and caps, and spots.

use super::McVec2;
use super::mesh::Mesh;
use super::{
    CAP_THETA, HALF_TURN_DEGREES, PI, TAU, Vec2, alpha, argb, blue, green, lerp, red, rotate,
};

/// A line's look: its width, its tail's and head's colors, and whether
/// it's colored from one to the other along its length (holding Ctrl in
/// the original) or the tail's throughout.
#[derive(Debug, Clone, Copy)]
pub struct Line {
    pub width: f32,
    pub tail: u32,
    pub head: u32,
    pub gradient: bool,
}

impl Line {
    /// A line one color throughout.
    pub const fn solid(width: f32, color: u32) -> Self {
        Line {
            width,
            tail: color,
            head: color,
            gradient: false,
        }
    }

    /// The color at `t` (0 the tail, 1 the head) of its length.
    fn color_at(&self, t: f32) -> u32 {
        let head = if self.gradient { self.head } else { self.tail };
        let mix = |c: fn(u32) -> i32| lerp(t, c(self.tail) as f32, c(head) as f32) as i32;
        argb(mix(alpha), mix(red), mix(green), mix(blue))
    }
}

/// `drawLineSeq`: a `line`-looking polyline through `points`, with round
/// joins and caps.
pub fn draw_line_seq(mesh: &mut Mesh, points: &[Vec2], line: Line) {
    let n = points.len();
    if n <= 1 {
        return;
    }
    let joins = Joins::of(points, line.width);
    for i in 0..n - 1 {
        let colors = (
            line.color_at(i as f32 / n as f32),
            line.color_at((i + 1) as f32 / n as f32),
        );
        segment(mesh, points, i, colors, &joins);
    }
    cap(mesh, line.width, line.color_at(0.0), points[0], points[1]);
    cap(
        mesh,
        line.width,
        line.color_at(1.0),
        points[n - 1],
        points[n - 2],
    );
}

/// A line's joins: its width, and at each point the angle the line turns
/// there and how far its segments' ends pull back for it.
pub(super) struct Joins {
    width: f32,
    at: Vec<(f32, f32)>,
}

impl Joins {
    fn of(points: &[Vec2], width: f32) -> Self {
        let mut at = vec![(0.0, 0.0); points.len()];
        for i in 2..points.len() {
            let (p0, p1, p2) = (points[i - 2], points[i - 1], points[i]);
            let prev = p1 - p0;
            let next = p2 - p1;
            let angle =
                (prev.x * next.y - prev.y * next.x).atan2(prev.x * next.x + prev.y * next.y);
            let clamp = prev.length().min(next.length()) / (width / 2.0);
            at[i - 1] = (
                angle,
                (angle.sin() / (1.0 + angle.cos())).clamp(-clamp, clamp),
            );
        }
        Joins { width, at }
    }
}

/// Segment `i` (to point `i + 1`), `colors` at its ends, and its join
/// to the segment before.
pub(super) fn segment(
    mesh: &mut Mesh,
    points: &[Vec2],
    i: usize,
    (color1, color2): (u32, u32),
    joins: &Joins,
) {
    let (p1, p2) = (points[i], points[i + 1]);
    let tangent = (p2 - p1).normalized() * (joins.width / 2.0);
    let normal = Vec2::new(-tangent.y, tangent.x);
    let (jlow, jhigh) = (joins.at[i].1, joins.at[i + 1].1);
    let p1_down = p1 + tangent * 0f32.max(jlow) + normal;
    let p1_up = p1 + tangent * 0f32.max(-jlow) - normal;
    let p2_down = p2 - tangent * 0f32.max(jhigh) + normal;
    let p2_up = p2 - tangent * 0f32.max(-jhigh) - normal;
    mesh.tri((color1, p1_down), (color1, p1), (color1, p1_up));
    mesh.tri((color1, p1_down), (color1, p1_up), (color2, p2_up));
    mesh.tri((color1, p1_down), (color2, p2_up), (color2, p2));
    mesh.tri((color1, p1_down), (color2, p2), (color2, p2_down));
    if i > 0 {
        join(mesh, p1, normal, joins.at[i].0, color1);
    }
}

/// The connector at `p` to the previous segment: a fan through the
/// `turn` the line takes there.
pub(super) fn join(mesh: &mut Mesh, p: Vec2, normal: Vec2, turn: f32, color: u32) {
    let steps = (turn.abs().to_degrees() / CAP_THETA).ceil() as i32;
    if steps < 1 {
        return;
    }
    let fan_at = |j: i32, from: Vec2| p - rotate(from, -turn * (j as f32 / steps as f32));
    let mut prev = if turn < 0.0 {
        p + normal
    } else {
        p - rotate(normal, -turn)
    };
    let order: Box<dyn Iterator<Item = i32>> = if turn < 0.0 {
        Box::new(1..=steps)
    } else {
        Box::new((0..steps).rev())
    };
    for j in order {
        let fan = if turn < 0.0 {
            fan_at(j, -normal)
        } else {
            fan_at(j, normal)
        };
        mesh.tri((color, p), (color, prev), (color, fan));
        prev = fan;
    }
}

/// A round cap at `point`, the line coming from `prev`.
pub(super) fn cap(mesh: &mut Mesh, width: f32, color: u32, point: Vec2, prev: Vec2) {
    let tangent = (point - prev).normalized() * (width / 2.0);
    let normal = Vec2::new(-tangent.y, tangent.x);
    let steps = (HALF_TURN_DEGREES / CAP_THETA).ceil() as i32;
    let rim: Vec<Vec2> = (0..=steps)
        .rev()
        .map(|j| point + rotate(normal, -PI * (j as f32 / steps as f32)))
        .collect();
    mesh.fan(color, point, &rim);
}

/// `drawSpot`: a little hexagon (Hex Casting's "circle").
pub fn draw_spot(mesh: &mut Mesh, point: Vec2, radius: f32, color: u32) {
    const FRAC: i32 = 6;
    let rim: Vec<Vec2> = (0..=FRAC)
        .map(|i| {
            let theta = i as f32 / FRAC as f32 * TAU as f32;
            Vec2::new(
                theta.cos() * radius + point.x,
                theta.sin() * radius + point.y,
            )
        })
        .collect();
    mesh.fan(color, point, &rim);
}
