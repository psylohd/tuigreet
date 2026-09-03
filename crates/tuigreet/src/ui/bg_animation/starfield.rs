//! Sparse drifting starfield.
//!
//! A fixed population of stars streams diagonally across the screen at
//! per-star speeds. When a star leaves the trailing edge it is
//! immediately respawned at the leading edge with a fresh speed and
//! brightness — the population is steady-state, so the effect is
//! genuinely continuous rather than a finite loop. Each star twinkles
//! by stochastically stepping its brightness up or down each frame.
//!
//! Density (count) and palette are configurable. The default density
//! targets ~3% of cells on an 80×24 terminal (~58 stars), which is dense
//! enough to feel alive but sparse enough that the login form remains
//! trivially legible.

use std::time::{SystemTime, UNIX_EPOCH};

use rand::{RngExt, SeedableRng, prelude::StdRng};
use tui::{
  buffer::Buffer,
  layout::{Position, Rect},
  style::Color,
};

use super::Animation;

/// Brightness bands used for twinkle. Index 0 is the dimmest, the last
/// is the brightest. The number of bands defines the visual "depth"
/// range — the actual color for each band is set by [`Options::palette`].
const BRIGHTNESS_BANDS: u8 = 4;

/// Glyph catalog. Each star picks one glyph at spawn and keeps it for
/// its lifetime; glyph variation gives the field its texture without
/// requiring per-frame animation of the symbol itself.
const GLYPHS: &[char] = &['·', '∙', '•', '✦', '✶', '⋆', '*'];

/// Configurable parameters for the starfield effect.
#[derive(Debug, Clone)]
pub struct Options {
  /// Target number of stars, clamped to `>= 1`. The actual count is
  /// `min(density, width * height)` so the field never over-saturates a
  /// tiny terminal.
  pub density:      u32,
  /// Inclusive minimum drift speed, in cells per frame. `0.0` is valid
  /// (the star barely moves) but the macro clamps to `>= 0.01`.
  pub min_speed:    f32,
  /// Inclusive maximum drift speed in cells per frame.
  pub max_speed:    f32,
  /// Per-frame, per-star probability of a brightness band shift. `0.0`
  /// freezes twinkle; `1.0` is full strobe.
  pub twinkle_rate: f32,
  /// Color ramp from dimmest band to brightest. Must be non-empty;
  /// shorter ramps compress the visible depth range, longer ramps give
  /// smoother gradient.
  pub palette:      Vec<Color>,
}

impl Default for Options {
  fn default() -> Self {
    Self {
      density:      60,
      min_speed:    0.10,
      max_speed:    0.60,
      twinkle_rate: 0.05,
      palette:      vec![
        Color::Rgb(0x55, 0x55, 0x66),
        Color::Rgb(0x99, 0x99, 0xAA),
        Color::Rgb(0xCC, 0xCC, 0xDD),
        Color::Rgb(0xFF, 0xFF, 0xFF),
      ],
    }
  }
}

/// One star. Fractional `x` allows smooth drift at sub-cell speeds.
/// `respawn_x` is set just off the leading edge so the new star eases
/// in rather than popping onto the screen.
#[derive(Clone, Copy, Debug)]
struct Star {
  /// Fractional column, may be outside the visible range while the star
  /// is in its respawn-easing window.
  x:          f32,
  /// Row.
  y:          i32,
  /// Cells per frame (positive: drifts to the right).
  speed:      f32,
  /// Current brightness band index (0..BRIGHTNESS_BANDS).
  brightness: u8,
  /// Glyph chosen at spawn.
  glyph:      char,
}

pub struct Starfield {
  width:  u16,
  height: u16,
  stars:  Vec<Star>,
  opts:   Options,
  rng:    StdRng,
}

impl Starfield {
  pub fn new(mut opts: Options) -> Self {
    opts.min_speed = opts.min_speed.max(0.01);
    opts.max_speed = opts.max_speed.max(opts.min_speed);
    opts.twinkle_rate = opts.twinkle_rate.clamp(0.0, 1.0);
    if opts.palette.is_empty() {
      opts.palette.push(Color::White);
    }
    opts.density = opts.density.max(1);

    let seed = SystemTime::now()
      .duration_since(UNIX_EPOCH)
      .map(|d| d.as_nanos() as u64)
      .unwrap_or(0);

    Self {
      width: 0,
      height: 0,
      stars: Vec::new(),
      opts,
      rng: StdRng::seed_from_u64(seed),
    }
  }

  /// Spawn a fresh star at the leading edge (left of the screen) on a
  /// random row. Called both for the initial population and whenever a
  /// star drifts off the trailing edge.
  fn spawn(&mut self) -> Star {
    let h = self.height.max(1) as i32;
    let speed_lo = (self.opts.min_speed * 1000.0) as u32;
    let speed_hi = (self.opts.max_speed * 1000.0) as u32;
    let speed =
      self.rng.random_range(speed_lo..=speed_hi.max(speed_lo)) as f32 / 1000.0;
    Star {
      // Start a couple of cells off the leading edge so the new star
      // eases in over a few frames instead of popping into view.
      x:          -self.rng.random_range(0.0..2.0),
      y:          self.rng.random_range(0..h),
      speed,
      brightness: self.rng.random_range(0..BRIGHTNESS_BANDS),
      glyph:      GLYPHS[self.rng.random_range(0..GLYPHS.len())],
    }
  }

  /// (Re)initialize the star population to match the configured density
  /// for the current terminal size. Called from `resize` whenever the
  /// area changes.
  fn init_stars(&mut self) {
    self.stars.clear();
    if self.width == 0 || self.height == 0 {
      return;
    }
    // Cap density by area so a 4×3 terminal doesn't get the same 60
    // stars as an 80×24 one.
    let max = u32::from(self.width) * u32::from(self.height);
    let count = self.opts.density.min(max) as usize;
    for _ in 0..count {
      let star = self.spawn();
      self.stars.push(star);
    }
  }

  fn resolve_color(&self, band: u8) -> Color {
    let palette = &self.opts.palette;
    let idx = (band.min(BRIGHTNESS_BANDS - 1) as usize).min(palette.len() - 1);
    palette[idx]
  }
}

impl Animation for Starfield {
  fn resize(&mut self, area: Rect) {
    if area.width == self.width
      && area.height == self.height
      && !self.stars.is_empty()
    {
      return;
    }
    self.width = area.width;
    self.height = area.height;
    self.init_stars();
  }

  fn step(&mut self) {
    if self.width == 0 || self.height == 0 || self.stars.is_empty() {
      return;
    }
    let w = self.width as f32;
    let h = self.height as i32;
    let palette_max = self.opts.palette.len() as u8;

    // Snapshot the count to release the borrow on `self.stars` between
    // iterations.
    let n = self.stars.len();
    for i in 0..n {
      let star = self.stars[i];
      let new_x = star.x + star.speed;

      // Drift off the trailing edge? Respawn at the leading edge. This
      // is what makes the effect continuous: the population never
      // decreases, the visual motion never resets.
      if new_x > w {
        let fresh = self.spawn();
        self.stars[i] = fresh;
        continue;
      }

      // Clamp vertical drift if the terminal shrank between frames.
      let clamped_y = if star.y >= h {
        h - 1
      } else if star.y < 0 {
        0
      } else {
        star.y
      };

      // Twinkle: stochastically nudge the brightness band.
      let new_brightness = if self.opts.twinkle_rate > 0.0
        && self.rng.random_bool(self.opts.twinkle_rate as f64)
      {
        // Step up or down by 1, biased toward staying in range.
        let direction = if self.rng.random_bool(0.5) { 1i8 } else { -1i8 };
        let band = (star.brightness as i16 + direction as i16)
          .clamp(0, palette_max as i16 - 1) as u8;
        band.min(BRIGHTNESS_BANDS - 1)
      } else {
        star.brightness
      };

      self.stars[i] = Star {
        x: new_x,
        y: clamped_y,
        speed: star.speed,
        brightness: new_brightness,
        glyph: star.glyph,
      };
    }
  }

  fn render(&self, area: Rect, buf: &mut Buffer) {
    if self.width == 0 || self.height == 0 {
      return;
    }
    for star in &self.stars {
      let x = star.x.floor() as i32;
      let y = star.y;
      if x < 0 || x >= self.width as i32 || y < 0 || y >= self.height as i32 {
        // Star is in its respawn-easing window — skip the cell; the
        // animation never paints a partial star, it just waits for the
        // next frame.
        continue;
      }
      let screen_x = area.x + x as u16;
      let screen_y = area.y + y as u16;
      if let Some(cell) = buf.cell_mut(Position { x: screen_x, y: screen_y }) {
        cell.set_char(star.glyph);
        cell.set_fg(self.resolve_color(star.brightness));
        cell.set_bg(Color::Reset);
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn fixed(opts: Options) -> Starfield {
    let mut s = Starfield::new(opts);
    // Replace the time-seeded RNG with a deterministic one so the
    // assertions below are stable.
    s.rng = StdRng::seed_from_u64(42);
    s
  }

  #[test]
  fn options_normalize_inverted_ranges() {
    let s = Starfield::new(Options {
      min_speed: 2.0,
      max_speed: 0.1,
      ..Options::default()
    });
    assert!(s.opts.max_speed >= s.opts.min_speed);
  }

  #[test]
  fn options_ensure_palette_non_empty() {
    let s = Starfield::new(Options {
      palette: vec![],
      ..Options::default()
    });
    assert!(!s.opts.palette.is_empty());
  }

  #[test]
  fn empty_area_is_a_no_op() {
    let mut s = fixed(Options::default());
    s.resize(Rect::new(0, 0, 0, 0));
    assert!(s.stars.is_empty());
    s.step();
    assert!(s.stars.is_empty());
  }

  #[test]
  fn density_caps_at_area() {
    let mut s = fixed(Options { density: 100_000, ..Options::default() });
    s.resize(Rect::new(0, 0, 8, 4));
    assert!(s.stars.len() <= 8 * 4);
  }

  #[test]
  fn starfield_keeps_population_steady() {
    // After many frames the population should be exactly the same — no
    // star gets permanently consumed. This is the "continuous, not a
    // loop" invariant in test form.
    let mut s = fixed(Options::default());
    s.resize(Rect::new(0, 0, 40, 12));
    let initial = s.stars.len();
    for _ in 0..500 {
      s.step();
    }
    assert_eq!(s.stars.len(), initial);
  }

  #[test]
  fn paints_into_buffer_after_some_steps() {
    let mut s = fixed(Options::default());
    s.resize(Rect::new(0, 0, 40, 20));
    for _ in 0..30 {
      s.step();
    }
    let mut buf = Buffer::empty(Rect::new(0, 0, 40, 20));
    s.render(Rect::new(0, 0, 40, 20), &mut buf);
    let painted = (0..40)
      .flat_map(|x| (0..20).map(move |y| (x, y)))
      .filter(|(x, y)| {
        let cell = &buf[(*x, *y)];
        let s = cell.symbol();
        s != " " && !s.is_empty()
      })
      .count();
    assert!(painted > 0, "starfield should paint something after 30 frames");
  }

  #[test]
  fn resize_scales_state() {
    let mut s = fixed(Options::default());
    s.resize(Rect::new(0, 0, 16, 8));
    let pop_16x8 = s.stars.len();
    s.resize(Rect::new(0, 0, 32, 16));
    let pop_32x16 = s.stars.len();
    assert!(pop_32x16 >= pop_16x8);
    assert_eq!(s.width, 32);
    assert_eq!(s.height, 16);
  }
}
