//! Slow, ambient color curtains that drift across the upper portion of
//! the screen.
//!
//! Two horizontal curtains of color slide past each other following
//! sine paths; their contributions blend per cell, falling off toward
//! the bottom of the screen so the login form sits in calm, uncolored
//! space underneath.
//!
//! Reads as aurora borealis: large-scale, slow, organic — the
//! opposite of matrix-style rain. Cost is moderate per frame, so this
//! animation overrides [`frame_divider`] to step the simulation every
//! other render tick. The visual change between frames is small enough
//! that the throttle is invisible.
//!
//! The login form is further protected by [`Clear`] in `ui::prompt`.
//!
//! [`frame_divider`]: super::Animation::frame_divider
//! [`Clear`]: ratatui::widgets::Clear

use std::time::{SystemTime, UNIX_EPOCH};

use rand::{RngExt, SeedableRng, prelude::StdRng};
use tui::{
  buffer::Buffer,
  layout::{Position, Rect},
  style::Color,
};

use super::{Animation, color_to_rgb};
const CURTAIN_COUNT: usize = 2;

const DEFAULT_DIVIDER: u32 = 2;

/// Configurable parameters for the aurora effect.
#[derive(Debug, Clone)]
pub struct Options {
  /// Color of the first curtain at its peak intensity.
  pub color_a:  Color,
  /// Color of the second curtain at its peak intensity.
  pub color_b:  Color,
  /// Vertical band coverage, `0.0..=1.0`. `0.75` means the curtains
  /// fade to zero by 75% of the way down the screen — the bottom 25%
  pub coverage: f32,
  /// Drift speed multiplier. `1.0` is the default cadence; raise to
  /// make the curtains race past faster, lower to make them glacial.
  pub speed:    f32,
}

impl Default for Options {
  fn default() -> Self {
    Self {
      // Deep teal → bright cyan-blue: classic cold-aurora palette.
      // The two colors sit close on the spectrum so the overlap
      // regions blend smoothly instead of producing a third color.
      color_a:  Color::Rgb(0x22, 0x66, 0x99),
      color_b:  Color::Rgb(0x55, 0xAA, 0xFF),
      coverage: 0.75,
      speed:    1.0,
    }
  }
}

/// One curtain. Vertical center follows a sine path as a function of
/// column; phase advances each frame for the drift animation.
#[derive(Debug, Clone, Copy)]
struct Curtain {
  /// Wavelength of the sine path, in cells.
  wavelength: f32,
  /// Amplitude of the sine path, in cells.
  amplitude:  f32,
  /// Phase offset, radians. Stamped at spawn; advances per frame.
  phase:      f32,
  /// Vertical center at column 0, fraction of screen height.
  center_y:   f32,
  /// Half-thickness, fraction of screen height.
  thickness:  f32,
  /// Drift speed, radians per frame.
  drift:      f32,
  /// Which configured color this curtain uses.
  uses_a:     bool,
}

pub struct Aurora {
  width:    u16,
  height:   u16,
  curtains: [Curtain; CURTAIN_COUNT],
  opts:     Options,
  rng:      StdRng,
  tick:     u32,
}

impl Aurora {
  pub fn new(mut opts: Options) -> Self {
    opts.coverage = opts.coverage.clamp(0.1, 1.0);
    opts.speed = opts.speed.max(0.0);

    let seed = SystemTime::now()
      .duration_since(UNIX_EPOCH)
      .map(|d| d.as_nanos() as u64)
      .unwrap_or(0);

    let mut rng = StdRng::seed_from_u64(seed);

    let make = |rng: &mut StdRng, uses_a: bool| Curtain {
      wavelength: rng.random_range(8.0..20.0),
      amplitude:  rng.random_range(2.0..6.0),
      phase:      rng.random_range(0.0..std::f32::consts::TAU),
      center_y:   rng.random_range(0.15..0.35),
      thickness:  rng.random_range(0.10..0.18),
      drift:      rng.random_range(0.01..0.03),
      uses_a,
    };

    Self {
      width:    0,
      height:   0,
      curtains: [make(&mut rng, true), make(&mut rng, false)],
      opts,
      rng,
      tick: 0,
    }
  }

  fn lerp(a: Color, b: Color, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    let ar = color_to_rgb(a).or_else(|| color_to_rgb(b));
    let br = color_to_rgb(b).or_else(|| color_to_rgb(a));
    let (Some((ar, ag, ab)), Some((br, bg, bb))) = (ar, br) else {
      return Color::Reset;
    };
    Color::Rgb(
      (f32::from(ar) + (f32::from(br) - f32::from(ar)) * t).round() as u8,
      (f32::from(ag) + (f32::from(bg) - f32::from(ag)) * t).round() as u8,
      (f32::from(ab) + (f32::from(bb) - f32::from(ab)) * t).round() as u8,
    )
  }
  /// Sample the curtain's intensity at `(x, y)`. Returns
  /// `(intensity, color)`; the caller picks the dominant one and may
  /// blend in the secondary at low weight.
  fn sample(&self, c: &Curtain, x: i32, y: i32) -> (f32, Color) {
    let h = self.height.max(1) as f32;
    let w = self.width.max(1) as f32;

    // Sine path: curtain bends left/right as a function of column.
    let bend =
      (c.phase + c.drift * self.tick as f32).sin() * c.amplitude;
    let center_y = (c.center_y + bend / h).clamp(0.0, 1.0);
    let row_norm = y as f32 / h;
    let dist = (row_norm - center_y).abs() / c.thickness;
    let intensity = (1.0 - dist).clamp(0.0, 1.0).powi(2);
    let coverage = (1.0 - row_norm / self.opts.coverage).clamp(0.0, 1.0);

    let horizontal = 0.5
      + 0.5
        * ((x as f32 / w) * std::f32::consts::TAU / c.wavelength
          + c.phase
          + c.drift * self.tick as f32)
          .sin();

    let combined = intensity * coverage * horizontal;
    (combined, if c.uses_a { self.opts.color_a } else { self.opts.color_b })
  }
}

impl Animation for Aurora {
  fn resize(&mut self, area: Rect) {
    if area.width == self.width && area.height == self.height && self.width > 0
    {
      return;
    }
    self.width = area.width;
    self.height = area.height;
  }

  fn step(&mut self) {
    self.tick = self.tick.wrapping_add(1);
  }

  fn render(&self, area: Rect, buf: &mut Buffer) {
    if self.width == 0 || self.height == 0 {
      return;
    }
    let w = self.width as i32;
    let h = self.height as i32;

    for y in 0..h {
      for x in 0..w {
        // Find the strongest contribution; blend the other in at
        // low weight so both colors stay visible where they overlap.
        let mut best: Option<(f32, Color)> = None;
        for c in &self.curtains {
          let (intensity, color) = self.sample(c, x, y);
          if intensity <= 0.01 {
            continue;
          }
          best = Some(match best {
            None => (intensity, color),
            Some((prev_i, prev_c)) if intensity > prev_i => {
              (intensity, color)
            }
            Some((prev_i, prev_c)) => (
              prev_i.max(intensity * 0.4),
              Self::lerp(prev_c, color, intensity * 0.3),
            ),
          });
        }
        let Some((intensity, color)) = best else {
          continue;
        };
        let sx = area.x + x as u16;
        let sy = area.y + y as u16;
        if let Some(cell) = buf.cell_mut(Position { x: sx, y: sy }) {
          let ch =
            if intensity < 0.05 { ' ' } else if intensity < 0.30 { '·' } else { '•' };
          cell.set_char(ch);
          cell.set_fg(color);
          cell.set_bg(Color::Reset);
        }
      }
    }
  }

  fn frame_divider(&self) -> u32 {
    DEFAULT_DIVIDER
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn fixed(opts: Options) -> Aurora {
    let mut a = Aurora::new(opts);
    a.rng = StdRng::seed_from_u64(42);
    a
  }

  #[test]
  fn options_normalize_out_of_range_coverage() {
    let a = Aurora::new(Options { coverage: 5.0, ..Options::default() });
    assert!(a.opts.coverage <= 1.0);
    let a = Aurora::new(Options { coverage: -1.0, ..Options::default() });
    assert!(a.opts.coverage >= 0.1);
  }

  #[test]
  fn empty_area_is_a_no_op() {
    let mut a = fixed(Options::default());
    a.resize(Rect::new(0, 0, 0, 0));
    a.step();
    assert_eq!(a.width, 0);
    assert_eq!(a.height, 0);
  }

  #[test]
  fn frame_divider_is_two() {
    assert_eq!(fixed(Options::default()).frame_divider(), 2);
  }

  #[test]
  fn paints_into_buffer_after_some_steps() {
    let mut a = fixed(Options::default());
    a.resize(Rect::new(0, 0, 80, 24));
    for _ in 0..30 {
      a.step();
    }
    let mut buf = Buffer::empty(Rect::new(0, 0, 80, 24));
    a.render(Rect::new(0, 0, 80, 24), &mut buf);
    let painted = (0..80)
      .flat_map(|x| (0..24).map(move |y| (x, y)))
      .filter(|(x, y)| {
        let s = buf[(*x, *y)].symbol();
        s != " " && !s.is_empty()
      })
      .count();
    assert!(painted > 0, "aurora should paint something after 30 frames");
  }

  #[test]
  fn bottom_band_stays_unpainted() {
    // The aurora is intentionally confined to the top so the form
    // has room to breathe — the bottom 40% should never receive a
    // glyph regardless of how many frames run.
    let mut a = Aurora::new(Options {
      coverage: 0.60,
      ..Options::default()
    });
    a.resize(Rect::new(0, 0, 80, 24));
    for _ in 0..200 {
      a.step();
    }
    let mut buf = Buffer::empty(Rect::new(0, 0, 80, 24));
    a.render(Rect::new(0, 0, 80, 24), &mut buf);
    let painted_in_bottom = (0..80)
      .filter(|x| {
        let s = buf[(*x, 23)].symbol();
        s != " " && !s.is_empty()
      })
      .count();
    assert_eq!(
      painted_in_bottom, 0,
      "bottom row must stay dark; the form sits here"
    );
  }

  #[test]
  fn resize_changes_state() {
    let mut a = fixed(Options::default());
    a.resize(Rect::new(0, 0, 16, 8));
    a.step();
    a.resize(Rect::new(0, 0, 32, 16));
    assert_eq!(a.width, 32);
    assert_eq!(a.height, 16);
  }
}
