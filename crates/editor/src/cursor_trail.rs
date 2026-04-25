//! Neovide-style smooth cursor animation.
//!
//! The cursor is drawn as a four-corner quadrilateral. When the caret jumps,
//! each corner animates independently toward its new destination using a
//! critically-damped spring. Corners aligned with the direction of travel get
//! a shorter animation length than trailing corners, which stretches the
//! cursor into a comet-like shape for the duration of the animation.
//!
//! Spring math and corner-ranking are ported from Neovide
//! (src/renderer/cursor_renderer, MIT-licensed).

use std::time::Instant;

use gpui::{Hsla, PathBuilder, Pixels, Point, Window, px};

#[inline]
fn pf(p: Pixels) -> f32 {
    p.into()
}

/// Paints the deformable cursor as a filled quadrilateral defined by 4 corners
/// (TL, TR, BR, BL).
pub fn paint_animated_quad(corners: [Point<Pixels>; 4], color: Hsla, window: &mut Window) {
    let mut builder = PathBuilder::fill();
    builder.move_to(corners[0]);
    builder.line_to(corners[1]);
    builder.line_to(corners[2]);
    builder.line_to(corners[3]);
    builder.line_to(corners[0]);
    if let Ok(path) = builder.build() {
        window.paint_path(path, color);
    }
}

/// Critically-damped spring. `position` is the offset from the target; the
/// spring decays it toward zero over `animation_length` seconds.
#[derive(Clone, Copy, Debug, Default)]
pub struct Spring {
    pub position: f32,
    pub velocity: f32,
}

impl Spring {
    /// Advances the spring. Returns `true` while still animating.
    pub fn update(&mut self, dt: f32, animation_length: f32) -> bool {
        if animation_length <= dt {
            self.position = 0.0;
            self.velocity = 0.0;
            return false;
        }
        if self.position == 0.0 && self.velocity == 0.0 {
            return false;
        }
        // Critically-damped harmonic oscillator, analytic solution.
        // omega chosen so the spring settles within ~2% in animation_length.
        let omega = 4.0 / animation_length;
        let a = self.position;
        let b = self.position * omega + self.velocity;
        let c = (-omega * dt).exp();
        self.position = (a + b * dt) * c;
        self.velocity = c * (-a * omega - b * dt * omega + b);
        if self.position.abs() < 0.01 {
            self.position = 0.0;
            self.velocity = 0.0;
            false
        } else {
            true
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Corner {
    pub spring_x: Spring,
    pub spring_y: Spring,
    pub animation_length: f32,
}

pub type CursorKey = (u32, u32);

/// Per-editor animation state for the newest cursor. Multi-cursor is not
/// animated — only the newest caret gets a trail.
#[derive(Default)]
pub struct CursorAnimationState {
    corners: [Corner; 4],
    last_key: Option<CursorKey>,
    last_destination: Option<[Point<Pixels>; 4]>,
    last_frame_at: Option<Instant>,
}

impl CursorAnimationState {
    /// Clears any in-progress animation. Used when the caret leaves the
    /// viewport or animation is disabled.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Returns the 4 animated corner positions for the cursor quad this frame,
    /// and whether the animation is still running (so the caller can schedule
    /// another frame). `destination` is the rect the cursor would occupy if
    /// drawn rigidly, as absolute screen-space corners in TL/TR/BR/BL order.
    pub fn step(
        &mut self,
        key: CursorKey,
        destination: [Point<Pixels>; 4],
        now: Instant,
        animation_length: f32,
        short_animation_length: f32,
        trail_size: f32,
    ) -> ([Point<Pixels>; 4], bool) {
        let dt = match self.last_frame_at {
            Some(prev) => now.saturating_duration_since(prev).as_secs_f32().min(0.1),
            None => 0.0,
        };
        self.last_frame_at = Some(now);

        let key_changed = self.last_key != Some(key);
        match (key_changed, self.last_destination) {
            (true, Some(prev)) => {
                // Re-anchor springs: preserve the on-screen visual position
                // while the destination moves to the new location.
                for i in 0..4 {
                    self.corners[i].spring_x.position +=
                        pf(destination[i].x - prev[i].x);
                    self.corners[i].spring_y.position +=
                        pf(destination[i].y - prev[i].y);
                }
                self.set_jump_animation_lengths(
                    destination,
                    animation_length,
                    short_animation_length,
                    trail_size,
                );
            }
            (true, None) => {
                // First observation after reset — snap, no animation.
                for corner in &mut self.corners {
                    corner.spring_x = Spring::default();
                    corner.spring_y = Spring::default();
                }
            }
            (false, Some(prev)) if prev != destination => {
                // Scroll (or other non-buffer movement): snap without trail.
                for corner in &mut self.corners {
                    corner.spring_x = Spring::default();
                    corner.spring_y = Spring::default();
                }
            }
            _ => {}
        }

        let mut animating = false;
        let mut painted = [Point::default(); 4];
        for i in 0..4 {
            let len = if self.corners[i].animation_length > 0.0 {
                self.corners[i].animation_length
            } else {
                animation_length
            };
            animating |= self.corners[i].spring_x.update(dt, len);
            animating |= self.corners[i].spring_y.update(dt, len);
            painted[i] = Point::new(
                destination[i].x - px(self.corners[i].spring_x.position),
                destination[i].y - px(self.corners[i].spring_y.position),
            );
        }

        self.last_key = Some(key);
        self.last_destination = Some(destination);
        (painted, animating)
    }

    fn set_jump_animation_lengths(
        &mut self,
        destination: [Point<Pixels>; 4],
        animation_length: f32,
        short_animation_length: f32,
        trail_size: f32,
    ) {
        let center_x = (pf(destination[0].x) + pf(destination[2].x)) * 0.5;
        let center_y = (pf(destination[0].y) + pf(destination[2].y)) * 0.5;
        let width = pf(destination[1].x - destination[0].x).abs().max(1.0);
        let height = pf(destination[2].y - destination[1].y).abs().max(1.0);

        // Current on-screen position of each corner.
        let visual: [Point<Pixels>; 4] = std::array::from_fn(|i| {
            Point::new(
                destination[i].x - px(self.corners[i].spring_x.position),
                destination[i].y - px(self.corners[i].spring_y.position),
            )
        });

        // Travel direction: average displacement across corners.
        let mut travel_x: f32 = 0.0;
        let mut travel_y: f32 = 0.0;
        for i in 0..4 {
            travel_x += pf(destination[i].x - visual[i].x);
            travel_y += pf(destination[i].y - visual[i].y);
        }
        let travel_len = (travel_x * travel_x + travel_y * travel_y).sqrt();
        let short_jump = travel_len < width * 2.001 && travel_y.abs() < height * 0.5;

        if short_jump {
            let len = animation_length.min(short_animation_length);
            for corner in &mut self.corners {
                corner.animation_length = len;
            }
            return;
        }

        // Rank corners by alignment with travel direction (most-aligned = rank 3).
        let travel_nx = if travel_len > 0.0 { travel_x / travel_len } else { 0.0 };
        let travel_ny = if travel_len > 0.0 { travel_y / travel_len } else { 0.0 };
        let mut alignments: [(usize, f32); 4] = std::array::from_fn(|i| {
            let cdx = pf(destination[i].x) - center_x;
            let cdy = pf(destination[i].y) - center_y;
            let cd_len = (cdx * cdx + cdy * cdy).sqrt().max(1.0);
            let cnx = cdx / cd_len;
            let cny = cdy / cd_len;
            (i, cnx * travel_nx + cny * travel_ny)
        });
        alignments.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        // alignments[0] = most trailing (rank 0), alignments[3] = most leading (rank 3).
        let leading = animation_length * (1.0 - trail_size).clamp(0.0, 1.0);
        let trailing = animation_length;
        let middle = (leading + trailing) * 0.5;
        let ranked_lengths = [trailing, middle, leading, leading];
        for (rank, (corner_idx, _)) in alignments.iter().enumerate() {
            self.corners[*corner_idx].animation_length = ranked_lengths[rank];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::point;
    use std::time::Duration;

    fn rect(x: f32, y: f32, w: f32, h: f32) -> [Point<Pixels>; 4] {
        [
            point(px(x), px(y)),
            point(px(x + w), px(y)),
            point(px(x + w), px(y + h)),
            point(px(x), px(y + h)),
        ]
    }

    #[test]
    fn first_observation_is_snap() {
        let mut state = CursorAnimationState::default();
        let dest = rect(10.0, 10.0, 8.0, 16.0);
        let (painted, animating) = state.step((0, 0), dest, Instant::now(), 0.15, 0.04, 0.8);
        assert!(!animating);
        assert_eq!(painted, dest);
    }

    #[test]
    fn scroll_only_snaps() {
        let mut state = CursorAnimationState::default();
        let t0 = Instant::now();
        state.step((0, 0), rect(10.0, 10.0, 8.0, 16.0), t0, 0.15, 0.04, 0.8);
        let (painted, animating) = state.step(
            (0, 0),
            rect(10.0, 50.0, 8.0, 16.0),
            t0 + Duration::from_millis(16),
            0.15,
            0.04,
            0.8,
        );
        assert!(!animating);
        assert_eq!(painted[0], point(px(10.0), px(50.0)));
    }

    #[test]
    fn buffer_jump_animates() {
        let mut state = CursorAnimationState::default();
        let t0 = Instant::now();
        state.step((0, 0), rect(10.0, 10.0, 8.0, 16.0), t0, 0.15, 0.04, 0.8);
        let (painted, animating) = state.step(
            (42, 0),
            rect(400.0, 400.0, 8.0, 16.0),
            t0 + Duration::from_millis(16),
            0.15,
            0.04,
            0.8,
        );
        assert!(animating);
        // The painted corners should be between old and new destination, not at new.
        assert!(pf(painted[0].x) < 400.0);
        assert!(pf(painted[0].y) < 400.0);
    }

    #[test]
    fn animation_settles() {
        let mut state = CursorAnimationState::default();
        let t0 = Instant::now();
        state.step((0, 0), rect(10.0, 10.0, 8.0, 16.0), t0, 0.15, 0.04, 0.8);
        state.step(
            (42, 0),
            rect(400.0, 400.0, 8.0, 16.0),
            t0 + Duration::from_millis(16),
            0.15,
            0.04,
            0.8,
        );
        // After a long time, the spring should be quiescent and painted = dest.
        let (painted, animating) = state.step(
            (42, 0),
            rect(400.0, 400.0, 8.0, 16.0),
            t0 + Duration::from_millis(1000),
            0.15,
            0.04,
            0.8,
        );
        assert!(!animating);
        assert_eq!(painted, rect(400.0, 400.0, 8.0, 16.0));
    }
}
