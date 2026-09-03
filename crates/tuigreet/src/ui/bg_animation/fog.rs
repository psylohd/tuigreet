//! Drifting, low-contrast fog made of breathing glyphs.
//!
//! Each cell samples a 2D scalar field driven by sine waves; the field
//! value picks a glyph from a small palette (` `, `·`, `:`, `-`, `~`)
//! and a brightness. Because the field is purely a function of
//! `(x, y, tick)` and uses cheap math, the whole screen repaints every
//! frame for very low CPU — no per-cell state, no RNG in the inner
//! loop.
//!
//! The palette is deliberately narrow (greys only) and the glyphs are
//! deliberately soft, so the fog reads as ambient atmosphere rather
//! than a foreground pattern. The login form sits in calm space below
//! the field's gradient; `Clear` in `ui::prompt` covers any leakage.
//!
//! Continuous, not a loop: the field's phase advances every tick and
//! the cells it visits never repeat, so the surface looks like a
//! living texture forever.

use std::time::{SystemTime, UNIX_EPOCH};

use tui::{
  buffer::Buffer,
  layout::{Position, Rect},
  style::Color,
};

use super::{Animation, color_to_rgb};

/// Glyphs ordered from lowest intensity to highest. Narrow palette on
/// purpose: the eye reads "atmosphere" rather than "screen of dots"
const GLYPHS: &[char] = &[' ', '·', ':'];

/// Configurable parameters for the fog effect.
#[derive(Debug, Clone)]
pub struct Options {
  /// Speed multiplier. `1.0` is the default cadence; raise for
  /// faster drift, lower for a more meditative feel.
  pub speed:    f32,
  /// Density of the field's spatial frequency. `1.0` is the default
  /// wavelength; lower values produce larger blobs, higher values
  /// produce finer texture.
  pub scale:    f32,
  /// Color of the dimmest visible glyph (the first non-space).
  pub dim:      Color,
  /// Color of the brightest glyph.
  pub bright:   Color,
}

impl Default for Options {
  fn default() -> Self {
    Self {
      speed:  1.0,
      scale:  1.0,
      dim:    Color::Rgb(0x44, 0x44, 0x55),
      bright: Color::Rgb(0xAA, 0xAA, 0xCC),
    }
  }
}

pub struct Fog {
  width: u16,
  height: u16,
  tick: u32,
  opts:  Options,
  /// Per-instance phase offset so two simultaneous instances don't
  /// draw identical patterns. Stamped once at construction; the
  /// animation has no other state.
  phase:  f32,
}

impl Fog {
  pub fn new(mut opts: Options) -> Self {
    opts.speed = opts.speed.max(0.0);
    opts.scale = opts.scale.max(0.05);
    let seed = SystemTime::now()
      .duration_since(UNIX_EPOCH)
      .map(|d| d.as_nanos() as u64)
      .unwrap_or(0);
    // Derive a deterministic phase from the nanosecond timestamp. We
    // want uniqueness across restarts, not unpredictability — so a
    // simple mix is fine.
    let phase = ((seed >> 17) as f32).rem_euclid(std::f32::consts::TAU);
    Self {
      width: 0,
      height: 0,
      tick: 0,
      opts,
      phase,
    }
  }

  /// Sample the field at integer cell `(x, y)`. Returns
  /// `(intensity, glyph_index)` where intensity is in `0.0..=1.0` and
  /// glyph_index picks from [`GLYPHS`].
  ///
  /// The field is a sum of three traveling sine waves at different
  /// angles; their interference produces organic blobs without
  fn sample(&self, x: i32, y: i32) -> (f32, usize) {
    let scale = self.opts.scale * 0.35; // base frequency reduced ~3x → larger blobs
    let t = self.tick as f32 * 0.015 * self.opts.speed; // ~3x slower drift

    // Three waves traveling in different directions at the reduced
    // spatial frequency so interference patterns form slow, large
    // blobs rather than tight ripples.
    let n1 = ((x as f32) * 0.18 * scale + t).sin();
    let n2 = ((y as f32) * 0.22 * scale - t * 0.7 + self.phase).sin();
    let n3 =
      ((x as f32 + y as f32) * 0.12 * scale + t * 1.3 + self.phase * 0.5)
        .sin();
    let v = (n1 + n2 + n3) / 3.0; // ∈ [-1, 1]
    let intensity = ((v + 1.0) * 0.5).clamp(0.0, 1.0); // ∈ [0, 1]

    // Most cells stay blank. Only the brighter peaks of the field
    // paint, so the fog reads as drifting wisps rather than a
    // uniform screen of dots.
    let glyph_index = if intensity < 0.40 {
      0
    } else if intensity < 0.70 {
      1
    } else {
      2
    };
    (intensity, glyph_index)
  }

  fn lerp(a: Color, b: Color, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    // Use the shared `color_to_rgb` helper so named colors (e.g.
    // `green`, `light-green`) are mapped through the xterm 16-color
    // palette rather than collapsing to (0, 0, 0). For `Reset` /
    // `Indexed` on one side, fall back to the other side; for both
    // sides unrepresentable, return Reset and let the cell paint
    // whatever the terminal default is.
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
}

impl Animation for Fog {
  fn resize(&mut self, area: Rect) {
    if area.width == self.width && area.height == self.height {
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
        let (intensity, gi) = self.sample(x, y);
        if gi == 0 {
          continue;
        }
        // Use intensity to also pick the brightness within the band,
        // so the transition between glyphs is smooth, not stepped.
        let band_intensity =
          (intensity - band_lo(gi)).max(0.0) / band_width(gi);
        let color = Self::lerp(self.opts.dim, self.opts.bright, band_intensity);
        let sx = area.x + x as u16;
        let sy = area.y + y as u16;
        if let Some(cell) = buf.cell_mut(Position { x: sx, y: sy }) {
          cell.set_char(GLYPHS[gi]);
          cell.set_fg(color);
          cell.set_bg(Color::Reset);
        }
      }
    }
  }
}

fn band_lo(gi: usize) -> f32 {
  match gi {
    1 => 0.40,
    2 => 0.70,
    _ => 0.0,
  }
}

fn band_width(gi: usize) -> f32 {
  match gi {
    1 => 0.30,
    2 => 0.30,
    _ => 1.0,
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn options_normalize_out_of_range() {
    let f = Fog::new(Options { speed: -5.0, scale: 0.0, ..Options::default() });
    assert!(f.opts.speed >= 0.0);
    assert!(f.opts.scale >= 0.05);
  }

  #[test]
  fn empty_area_is_a_no_op() {
    let mut f = Fog::new(Options::default());
    f.resize(Rect::new(0, 0, 0, 0));
    f.step();
    assert_eq!(f.width, 0);
    assert_eq!(f.height, 0);
  }

  #[test]
  fn glyphs_are_all_width_one() {
    for &g in GLYPHS {
      let w = unicode_width::UnicodeWidthChar::width(g).unwrap_or(0);
      assert_eq!(w, 1, "glyph {g:?} has width {w}, must be 1");
    }
  }

  #[test]
  fn paints_into_buffer_after_some_steps() {
    let mut f = Fog::new(Options::default());
    f.resize(Rect::new(0, 0, 80, 24));
    for _ in 0..10 {
      f.step();
    }
    let mut buf = Buffer::empty(Rect::new(0, 0, 80, 24));
    f.render(Rect::new(0, 0, 80, 24), &mut buf);
    let painted = (0..80)
      .flat_map(|x| (0..24).map(move |y| (x, y)))
      .filter(|(x, y)| {
        let s = buf[(*x, *y)].symbol();
        s != " " && !s.is_empty()
      })
      .count();
    assert!(painted > 0, "fog should paint something after 10 frames");
  }

  #[test]
  fn field_is_continuous_across_steps() {
    // Two samples at adjacent ticks should produce close-but-not-
    // identical intensity distributions. This is the "continuous,
    // not a loop" invariant for a stateless sampler: the surface
    // changes smoothly, never resets.
    let mut f = Fog::new(Options::default());
    f.resize(Rect::new(0, 0, 40, 12));
    f.step();
    let mut a = Vec::new();
    for y in 0..12 {
      for x in 0..40 {
        a.push(f.sample(x, y).0);
      }
    }
    f.step();
    let mut b = Vec::new();
    for y in 0..12 {
      for x in 0..40 {
        b.push(f.sample(x, y).0);
      }
    }
    let diff: f32 = a
      .iter()
      .zip(b.iter())
      .map(|(x, y)| (x - y).abs())
      .sum::<f32>()
      / a.len() as f32;
    assert!(
      diff > 0.001,
      "field didn't change between adjacent ticks: {diff}"
    );
    assert!(
      diff < 0.2,
      "field changed too dramatically between adjacent ticks: {diff}"
    );
  }

  /// Regression test: when the user configures named colors like
  /// `green` or `light-green` in the TOML, the fog must interpolate
  /// between them in the green family — not collapse to black. The
  /// pre-fix `lerp` returned `(0, 0, 0)` for any non-`Color::Rgb`
  /// input, which made every fog render with named colors look
  /// uniformly black.
  #[test]
  fn renders_named_colors_without_collapsing_to_black() {
    let mut f = Fog::new(Options {
      dim:    Color::Green,
      bright: Color::LightGreen,
      ..Options::default()
    });
    f.resize(Rect::new(0, 0, 80, 24));
    for _ in 0..10 {
      f.step();
    }
    let mut buf = Buffer::empty(Rect::new(0, 0, 80, 24));
    f.render(Rect::new(0, 0, 80, 24), &mut buf);

    // Find any painted cell and check its foreground is greenish.
    let mut painted: Option<tui::style::Color> = None;
    'outer: for y in 0..24 {
      for x in 0..80 {
        if buf[(x, y)].symbol() != " " {
          painted = Some(buf[(x, y)].fg);
          break 'outer;
        }
      }
    }
    let fg =
      painted.expect("fog should paint something with named colors");
    match fg {
      Color::Rgb(_, g, _) => assert!(
        g >= 64,
        "fog painted with named colors should be greenish, got {fg:?}"
      ),
      other => panic!(
        "fog painted a non-RGB color from named inputs: {other:?}; \
         color_to_rgb was not applied"
      ),
    }
  }
}
