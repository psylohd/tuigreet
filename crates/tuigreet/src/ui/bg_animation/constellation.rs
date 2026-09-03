//! A small, stable constellation map whose connecting lines shimmer
//! on and off as if seen through atmosphere.
//!
//! Each constellation is a fixed set of stars (dots) connected by a
//! fixed set of edges. The whole topology is generated once at
//! [`resize`](Animation::resize) and never changes — the user
//! gradually memorizes it within seconds and then ignores it
//! peripherally, which is exactly what "ambient" wants. The shimmer
//! runs over the edges, not the stars: each edge has an independent
//! brightness pulse with its own phase and period, so the pattern
//! looks organic instead of strobing in lockstep.
//!
//! Stars are drawn as `·` or `•` (single-cell width-1); edges as
//! box-drawing characters (`─`, `│`, `╱`, `╲`) — also width-1. Every
//! drawn cell is `set_char`-only.
//!
//! Density is intentionally low: ~5 constellations × ~6 stars × ~7
//! edges = a few hundred painted cells on an 80×24 terminal, far
//! below the "distracting" threshold. The login form sits in calm
//! space below the constellation region; even if it didn't,
//! `Clear` in `ui::prompt` would wipe any leakage.

use std::time::{SystemTime, UNIX_EPOCH};

use rand::{RngExt, SeedableRng, prelude::StdRng};
use tui::{
  buffer::Buffer,
  layout::{Position, Rect},
  style::Color,
};

use super::Animation;

const CONSTELLATION_COUNT: usize = 5;
const STARS_PER_CONSTELLATION: usize = 10;
const EDGES_PER_CONSTELLATION: usize = 14;
/// One edge in a constellation. Carries its own shimmer pulse so
/// edges don't strobe in lockstep across the map.
#[derive(Clone, Copy, Debug)]
struct Edge {
  from:  u8,
  to:    u8,
  phase: f32,
  /// Pulse period, in render ticks. Longer = slower breathing.
  period: f32,
}

/// One constellation: a fixed set of stars and connecting edges.
#[derive(Clone, Debug)]
struct Group {
  stars: Vec<(i32, i32)>,
  edges: Vec<Edge>,
}

#[derive(Debug, Clone)]
pub struct Options {
  /// Color of the static star dots.
  pub star_color: Color,
  /// Color of fully-bright edges.
  pub edge_color: Color,
  /// Color that a fully-dimmed edge fades to.
  pub dim_color:  Color,
  /// Speed multiplier for the shimmer pulse.
  pub speed:      f32,
}

impl Default for Options {
  fn default() -> Self {
    Self {
      star_color: Color::Rgb(0xCC, 0xCC, 0xFF),
      edge_color: Color::Rgb(0x88, 0xAA, 0xFF),
      dim_color:  Color::Rgb(0x33, 0x33, 0x55),
      speed:      1.0,
    }
  }
}

pub struct Constellation {
  width:  u16,
  height: u16,
  groups: Vec<Group>,
  opts:   Options,
  rng:    StdRng,
  tick:   u32,
}

impl Constellation {
  pub fn new(mut opts: Options) -> Self {
    opts.speed = opts.speed.max(0.0);
    let seed = SystemTime::now()
      .duration_since(UNIX_EPOCH)
      .map(|d| d.as_nanos() as u64)
      .unwrap_or(0);
    Self {
      width:  0,
      height: 0,
      groups: Vec::new(),
      opts,
      rng: StdRng::seed_from_u64(seed),
      tick: 0,
    }
  }

  fn edge_intensity(&self, edge: &Edge) -> f32 {
    let t = (self.tick % 10_000) as f32 * self.opts.speed;
    let phase = edge.phase + t * std::f32::consts::TAU / edge.period;
    let s = 0.5 + 0.5 * phase.sin();
    s.powi(2)
  }

  fn lerp(a: Color, b: Color, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    let to_f = |c: Color| match c {
      Color::Rgb(r, g, b) => (f32::from(r), f32::from(g), f32::from(b)),
      _ => (0.0, 0.0, 0.0),
    };
    let (ar, ag, ab) = to_f(a);
    let (br, bg, bb) = to_f(b);
    Color::Rgb(
      (ar + (br - ar) * t).round() as u8,
      (ag + (bg - ag) * t).round() as u8,
      (ab + (bb - ab) * t).round() as u8,
    )
  }

  /// Rasterize a line segment between integer endpoints using
  /// Bresenham's algorithm. All emitted cells are width-1.
  fn bresenham(x0: i32, y0: i32, x1: i32, y1: i32, out: &mut Vec<(i32, i32)>) {
    let dx = (x1 - x0).abs();
    let dy = -(y1 - y0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;
    let mut x = x0;
    let mut y = y0;
    loop {
      out.push((x, y));
      if x == x1 && y == y1 {
        break;
      }
      let e2 = 2 * err;
      if e2 >= dy {
        err += dy;
        x += sx;
      }
      if e2 <= dx {
        err += dx;
        y += sy;
      }
    }
  }
}

fn pick_glyph(dx: i32, dy: i32) -> char {
  if dx == 0 {
    '│'
  } else if dy == 0 {
    '─'
  } else if (dx > 0) == (dy > 0) {
    '╲'
  } else {
    '╱'
  }
}

impl Animation for Constellation {
  fn resize(&mut self, area: Rect) {
    if area.width == self.width
      && area.height == self.height
      && !self.groups.is_empty()
    {
      return;
    }
    self.width = area.width;
    self.height = area.height;
    self.tick = 0;

    if area.width == 0 || area.height == 0 {
      self.groups.clear();
      return;
    }
    let w = area.width as i32;
    let h = area.height as i32;
    let y_max = (h as f32 * 0.70).max(4.0) as i32;

    self.groups.clear();
    for _ in 0..CONSTELLATION_COUNT {
      let cx = self.rng.random_range(2..w.saturating_sub(2).max(3));
      let cy = self.rng.random_range(1..y_max.saturating_sub(1).max(2));
      let rx = self.rng.random_range(8..14).min(w / 2);
      let ry = self.rng.random_range(4..6).min(y_max / 2);
      let mut stars: Vec<(i32, i32)> =
        Vec::with_capacity(STARS_PER_CONSTELLATION);
      for _ in 0..STARS_PER_CONSTELLATION {
        let sx = (cx + self.rng.random_range(-rx..=rx))
          .clamp(0, w.saturating_sub(1));
        let sy = (cy + self.rng.random_range(-ry..=ry))
          .clamp(0, y_max.saturating_sub(1));
        stars.push((sx, sy));
      }

      let mut edges: Vec<Edge> = Vec::with_capacity(EDGES_PER_CONSTELLATION);
      for _ in 0..EDGES_PER_CONSTELLATION {
        let a = self.rng.random_range(0..stars.len() as u8);
        let mut b = self.rng.random_range(0..stars.len() as u8);
        if b == a {
          b = (b + 1) % stars.len() as u8;
        }
        edges.push(Edge {
          from:  a,
          to:    b,
          phase: self.rng.random_range(0.0..std::f32::consts::TAU),
          period: self.rng.random_range(60.0..180.0),
        });
      }
      self.groups.push(Group { stars, edges });
    }
  }

  fn step(&mut self) {
    self.tick = self.tick.wrapping_add(1);
  }

  fn render(&self, area: Rect, buf: &mut Buffer) {
    if self.width == 0 || self.height == 0 || self.groups.is_empty() {
      return;
    }
    let w = self.width as i32;

    for g in &self.groups {
      // Edges first — painted underneath the stars so the star dot
      // visually "caps" the line at each endpoint.
      for edge in &g.edges {
        let intensity = self.edge_intensity(edge);
        // Always paint, even at very low intensity — shimmer is a
        // continuous brightness modulation, not an on/off flicker.
        // The dim_color blends toward dim_color at intensity=0, so
        // quiet edges are still visible against a black background.
        let color =
          Self::lerp(self.opts.dim_color, self.opts.edge_color, intensity);
        let (x0, y0) = g.stars[edge.from as usize];
        let (x1, y1) = g.stars[edge.to as usize];
        let mut cells = Vec::with_capacity(8);
        Self::bresenham(x0, y0, x1, y1, &mut cells);
        for (x, y) in cells {
          if x < 0 || x >= w || y < 0 || y >= self.height as i32 {
            continue;
          }
          let glyph = pick_glyph(x1 - x0, y1 - y0);
          let sx = area.x + x as u16;
          let sy = area.y + y as u16;
          if let Some(cell) = buf.cell_mut(Position { x: sx, y: sy }) {
            cell.set_char(glyph);
            cell.set_fg(color);
            cell.set_bg(Color::Reset);
          }
        }
      }

      for &(sx, sy) in &g.stars {
        if sx < 0 || sx >= w || sy < 0 || sy >= self.height as i32 {
          continue;
        }
        let ax = area.x + sx as u16;
        let ay = area.y + sy as u16;
        if let Some(cell) = buf.cell_mut(Position { x: ax, y: ay }) {
          let ch = if (sx + sy) & 1 == 0 { '•' } else { '·' };
          cell.set_char(ch);
          cell.set_fg(self.opts.star_color);
          cell.set_bg(Color::Reset);
        }
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn fixed(opts: Options) -> Constellation {
    let mut c = Constellation::new(opts);
    c.rng = StdRng::seed_from_u64(42);
    c
  }

  #[test]
  fn options_normalize_negative_speed() {
    let c = Constellation::new(Options { speed: -5.0, ..Options::default() });
    assert!(c.opts.speed >= 0.0);
  }

  #[test]
  fn empty_area_is_a_no_op() {
    let mut c = fixed(Options::default());
    c.resize(Rect::new(0, 0, 0, 0));
    assert!(c.groups.is_empty());
    c.step();
    assert!(c.groups.is_empty());
  }

  #[test]
  fn resize_generates_expected_count() {
    let mut c = fixed(Options::default());
    c.resize(Rect::new(0, 0, 80, 24));
    assert_eq!(c.groups.len(), CONSTELLATION_COUNT);
    for g in &c.groups {
      assert_eq!(g.stars.len(), STARS_PER_CONSTELLATION);
      assert_eq!(g.edges.len(), EDGES_PER_CONSTELLATION);
    }
  }

  #[test]
  fn stars_stay_in_upper_band() {
    // The form sits in the lower 30%; stars must never appear there
    // regardless of RNG seed.
    let y_max_seen = (0..10)
      .map(|seed| {
        let mut c = Constellation::new(Options::default());
        c.rng = StdRng::seed_from_u64(seed);
        c.resize(Rect::new(0, 0, 80, 24));
        c
          .groups
          .iter()
          .flat_map(|g| g.stars.iter().map(|&(_, y)| y))
          .max()
          .unwrap_or(0)
      })
      .max()
      .unwrap_or(0);
    assert!(y_max_seen <= 16, "stars leaked into form area: max row {y_max_seen}");
  }

  #[test]
  fn topology_is_stable_across_steps() {
    let mut c = fixed(Options::default());
    c.resize(Rect::new(0, 0, 80, 24));
    let before: Vec<_> = c
      .groups
      .iter()
      .map(|g| (g.stars.clone(), g.edges.len()))
      .collect();
    for _ in 0..100 {
      c.step();
    }
    let after: Vec<_> = c
      .groups
      .iter()
      .map(|g| (g.stars.clone(), g.edges.len()))
      .collect();
    assert_eq!(before, after);
  }

  #[test]
  fn paints_into_buffer_after_some_steps() {
    let mut c = fixed(Options::default());
    c.resize(Rect::new(0, 0, 80, 24));
    for _ in 0..30 {
      c.step();
    }
    let mut buf = Buffer::empty(Rect::new(0, 0, 80, 24));
    c.render(Rect::new(0, 0, 80, 24), &mut buf);
    let painted = (0..80)
      .flat_map(|x| (0..24).map(move |y| (x, y)))
      .filter(|(x, y)| {
        let s = buf[(*x, *y)].symbol();
        s != " " && !s.is_empty()
      })
      .count();
    assert!(
      painted > 0,
      "constellation should paint something after 30 frames"
    );
  }
}
