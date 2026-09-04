//! Background animations rendered behind the login UI.
//!
//! # ANSI-safety contract for implementors
//!
//! Ratatui renders by diffing the previous buffer against the new one and
//! emitting the smallest ANSI sequence per changed cell. Violating any of
//! the rules below corrupts that diff and produces visible glitches — or, on
//! a real TTY, escape sequences that no longer match the buffer state,
//! which is how a login screen ends up unusable.
//!
//! - **One glyph per cell.** Use only single-cell codepoints
//!   (`unicode-width == 1`). Half-width katakana (`U+FF65..U+FF9F`), basic
//!   Latin, box-drawing, and the block-drawing range used by the DOOM fire
//!   are all safe; emoji, CJK, and full-width Latin are not.
//! - **`set_char` only.** Never call `set_string` — it does not validate
//!   embedded escape characters and can write raw `\x1b` into a cell.
//! - **Stay inside `area`.** The `Rect` passed to `render` is the only
//!   area you may touch; writes outside it collide with the surrounding
//!   chrome (status bar, time row, padding).
//! - **Skip empty cells, do not paint "transparent" markers.** Either set
//!   a glyph or do nothing. Cells you skip are inherited from the
//!   previous frame's buffer, which is what lets `Clear` later wipe the
//!   login form without the animation reappearing through it.
//! - **`Color::Reset` means "terminal default".** On consoles with a black
//!   default background, a `Reset`-bg cell looks like a hole. Use it
//!   intentionally, not as a stand-in for a real color.
//! - **State across `resize`.** You may carry state (positions, ages) but
//!   must scale it to the new area rather than re-seed, or the effect
//!   visibly resets on every terminal resize.
//!
//! # Adding a new animation
//!
//! Add a single line to the [`bg_animation!`] table at the bottom of this
//! file and a submodule alongside `doom.rs` / `matrix.rs` that exposes
//! `<Name>::new(Options)` and `impl Animation for <Name>`. Everything else
//! (the `Kind` enum, `KINDS` table, `from_name`, `AnimationSpec`, `build`,
//! `default_spec`) is generated automatically.
//!
//! Format: `Display { module => path::Options, "config_name", "menu label" }`

use std::str::FromStr;

use tui::{buffer::Buffer, layout::Rect, style::Color};

/// Declare every animation this module exports.
///
/// Each row wires a submodule (`doom`, `matrix`, …) together with the
/// name used in config (`"doom"`, `"matrix"`, …) and the label shown in
/// the F-key switcher menu. The macro generates the `Kind` enum,
/// `KINDS` catalog, `Kind::from_name`, `AnimationSpec`, `build`, and
/// `default_spec` — so adding an animation only ever needs one new
/// submodule and one new line here.
///
/// Convention: each submodule contains a struct with the same PascalCase
/// name as the config key (e.g. module `doom` contains `struct Doom`).
/// The macro splices `$module::$name` to reach the constructor.
///
/// Single-name-per-kind on purpose: `macro_rules!` can't disambiguate
/// "variable number of literals then one terminator literal" without
/// recursion. If you need aliases later, switch to a `macro_rules!`
/// inner helper that consumes one literal at a time, or use a
/// declarative macro crate.
macro_rules! bg_animation {
  ($($name:ident { $module:ident => $opts:ty, $cfg_name:literal, $label:literal }),+ $(,)?) => {
    $(
      pub mod $module;
    )+

    /// Which animation to run.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Kind {
      $($name,)+
    }

    /// Catalog entry for a registered animation kind.
    #[allow(dead_code)]
    pub struct KindInfo {
      pub kind:  Kind,
      pub name:  &'static str,
      pub label: &'static str,
    }

    /// Every registered animation kind, in menu display order.
    pub const KINDS: &[KindInfo] = &[
      $(KindInfo { kind: Kind::$name, name: $cfg_name, label: $label, }),+
    ];

    impl Kind {
      /// Resolve a config string to a [`Kind`], or `None` for unknown / disabled.
      pub fn from_name(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
          $($cfg_name => Some(Self::$name),)+
          _ => None,
        }
      }
    }

    /// Fully-resolved configuration for one animation.
    #[derive(Debug, Clone)]
    pub enum AnimationSpec {
      $($name($opts),)+
    }

    /// Construct an animation matching `spec`'s variant.
    pub fn build(spec: &AnimationSpec) -> Box<dyn Animation> {
      match spec {
        $(AnimationSpec::$name(opts) => Box::new($module::$name::new(opts.clone())),)+
      }
    }

    impl Kind {
      /// Build a spec for this kind using its `Options::default()`.
      #[must_use]
      pub fn default_spec(self) -> AnimationSpec {
        match self {
          $(Self::$name => AnimationSpec::$name(<$opts>::default()),)+
        }
      }
    }
  };
}

bg_animation! {
  Doom      { doom      => doom::Options,     "doom",     "DOOM Fire" },
  Matrix    { matrix    => matrix::Options,   "matrix",   "Matrix"    },
  Starfield { starfield => starfield::Options,"starfield","Starfield" },
  Fog       { fog       => fog::Options,      "fog",      "Fog"       },
}

/// A background animation drawn beneath the login UI.
///
/// See the [module-level documentation](self) for the ANSI-safety
/// contract every implementor must honor.
pub trait Animation: Send + Sync {
  /// React to the current terminal size.
  fn resize(&mut self, area: Rect);

  /// Advance the simulation by one frame.
  ///
  /// Called every render tick by default. Override [`frame_divider`] if
  /// `N` and the caller will skip `step()` for `N - 1` ticks between
  /// simulations while still re-`render()`ing every tick (so the screen
  /// stays fresh from the previous step without extra CPU cost).
  fn step(&mut self);

  /// Called when the user provides input (key press). Animations can use
  /// this to boost their activity — e.g. a starfield speeds up when the
  /// user types.
  fn on_activity(&mut self) {}

  /// Paint the current frame.
  fn render(&self, area: Rect, buf: &mut Buffer);
  /// How often `step()` should actually advance, in render ticks. `1`
  /// means every tick (the default). Higher values throttle the
  /// simulation without changing the render cadence.
  #[must_use]
  fn frame_divider(&self) -> u32 {
    1
  }
}

/// Build an animation of the given kind using its default options.
pub fn build_default(kind: Kind) -> Box<dyn Animation> {
  build(&kind.default_spec())
}

/// Parse a color from `#RRGGBB`, `0xRRGGBB`, or any string accepted by
/// ratatui's [`Color::from_str`].
///
/// Hyphenated and underscored names are also accepted and normalized to
/// their no-separator form before being passed to ratatui. This lets
/// configs use familiar spellings like `light-green`, `dark-gray`, or
/// `light_blue` alongside ratatui's native `lightgreen` / `darkgray`.
pub fn parse_color(s: &str) -> Option<Color> {
  let trimmed = s.trim();
  let hex = trimmed
    .strip_prefix('#')
    .or_else(|| trimmed.strip_prefix("0x"))
    .or_else(|| trimmed.strip_prefix("0X"));
  if let Some(hex) = hex
    && hex.len() == 6
    && let Ok(r) = u8::from_str_radix(&hex[0..2], 16)
    && let Ok(g) = u8::from_str_radix(&hex[2..4], 16)
    && let Ok(b) = u8::from_str_radix(&hex[4..6], 16)
  {
    return Some(Color::Rgb(r, g, b));
  }
  if let Ok(color) = Color::from_str(trimmed) {
    return Some(color);
  }
  // Hyphenated / underscored fallbacks: strip the separators and try
  // again. Accepts "light-green", "light_green", "LIGHT-GREEN" → "lightgreen".
  let normalized: String = trimmed
    .chars()
    .filter(|c| !matches!(c, '-' | '_'))
    .collect::<String>()
    .to_lowercase();
  Color::from_str(&normalized).ok()
}

/// Convert any [`Color`] to its 24-bit RGB equivalent, using the xterm
/// 16-color palette for the named variants. Returns `None` for
/// `Color::Reset` and `Color::Indexed` — those have no fixed RGB value
/// (`Indexed` would need the xterm-256 palette, which ratatui does not
/// expose; `Reset` is whatever the terminal happens to be). Callers
/// should treat `None` as "use the other operand" when interpolating
/// between two colors.
///
/// The named-color RGB values are the standard xterm 16-color palette
/// that nearly every terminal uses as the rendering target for ANSI
/// color escapes. Picking these values keeps `light-green` looking like
/// `light-green` rather than collapsing to black inside color-mixing
/// animations like fog.
pub fn color_to_rgb(c: Color) -> Option<(u8, u8, u8)> {
  match c {
    Color::Rgb(r, g, b) => Some((r, g, b)),
    Color::Black => Some((0, 0, 0)),
    Color::Red => Some((128, 0, 0)),
    Color::Green => Some((0, 128, 0)),
    Color::Yellow => Some((128, 128, 0)),
    Color::Blue => Some((0, 0, 128)),
    Color::Magenta => Some((128, 0, 128)),
    Color::Cyan => Some((0, 128, 128)),
    Color::Gray => Some((192, 192, 192)),
    Color::DarkGray => Some((128, 128, 128)),
    Color::LightRed => Some((255, 0, 0)),
    Color::LightGreen => Some((0, 255, 0)),
    Color::LightYellow => Some((255, 255, 0)),
    Color::LightBlue => Some((0, 0, 255)),
    Color::LightMagenta => Some((255, 0, 255)),
    Color::LightCyan => Some((0, 255, 255)),
    Color::White => Some((255, 255, 255)),
    Color::Reset | Color::Indexed(_) => None,
  }
}

 #[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn parses_hex_colors() {
    assert_eq!(parse_color("#9F2707"), Some(Color::Rgb(0x9F, 0x27, 0x07)));
    assert_eq!(parse_color("0xFFFFFF"), Some(Color::Rgb(255, 255, 255)));
    assert_eq!(parse_color("0X000000"), Some(Color::Rgb(0, 0, 0)));
  }

  #[test]
  fn falls_back_to_named_colors() {
    assert!(parse_color("red").is_some());
  }

  #[test]
  fn rejects_garbage() {
    assert_eq!(parse_color("not-a-color"), None);
    assert_eq!(parse_color("#ZZZZZZ"), None);
  }

  #[test]
  fn parses_hyphenated_color_names() {
    // ratatui's native form (no hyphen) is the canonical one; the
    // hyphenated / underscored forms must collapse to the same color.
    let expected = parse_color("lightgreen");
    assert_eq!(parse_color("light-green"), expected);
    assert_eq!(parse_color("light_green"), expected);
    assert_eq!(parse_color("LIGHT-GREEN"), expected);
    assert_eq!(parse_color("Light_Green"), expected);
    // Other ANSI names with light/dark variants follow the same rule.
    let dark_gray = parse_color("darkgray");
    assert_eq!(parse_color("dark-gray"), dark_gray);
    assert_eq!(parse_color("dark_gray"), dark_gray);
  }

  #[test]
  fn hyphenated_garbage_still_rejected() {
    // After the separator strip, the result must still be a real
    // ratatui color name; otherwise we return None.
    assert_eq!(parse_color("not-a-color"), None);
    assert_eq!(parse_color("definitely_not_a_color"), None);
  }

  #[test]
  fn kind_from_name() {
    assert_eq!(Kind::from_name("doom"), Some(Kind::Doom));
    assert_eq!(Kind::from_name("DOOM"), Some(Kind::Doom));
    assert_eq!(Kind::from_name("fire"), None);
    assert_eq!(Kind::from_name("none"), None);
    assert_eq!(Kind::from_name(""), None);
    assert_eq!(Kind::from_name("matrix"), Some(Kind::Matrix));
    assert_eq!(Kind::from_name("CMATRIX"), None);
    assert_eq!(Kind::from_name("STARFIELD"), Some(Kind::Starfield));
  }

  #[test]
  fn color_to_rgb_named_palette() {
    // The named-color RGB values must match the xterm 16-color palette.
    assert_eq!(color_to_rgb(Color::Green), Some((0, 128, 0)));
    assert_eq!(color_to_rgb(Color::LightGreen), Some((0, 255, 0)));
    assert_eq!(color_to_rgb(Color::LightRed), Some((255, 0, 0)));
    assert_eq!(color_to_rgb(Color::Gray), Some((192, 192, 192)));
    assert_eq!(color_to_rgb(Color::DarkGray), Some((128, 128, 128)));
    assert_eq!(color_to_rgb(Color::White), Some((255, 255, 255)));
    assert_eq!(color_to_rgb(Color::Black), Some((0, 0, 0)));
  }

  #[test]
  fn color_to_rgb_rgb_passthrough() {
    assert_eq!(color_to_rgb(Color::Rgb(0x12, 0x34, 0x56)), Some((0x12, 0x34, 0x56)));
  }

  #[test]
  fn color_to_rgb_reset_and_indexed_have_no_rgb() {
    assert_eq!(color_to_rgb(Color::Reset), None);
    assert_eq!(color_to_rgb(Color::Indexed(42)), None);
  }


  #[test]
  fn all_kinds_honor_ansi_safety_contract() {
    use unicode_width::UnicodeWidthChar;

    for info in KINDS {
      let mut anim = build_default(info.kind);
      // Empty area must be a no-op, not a panic.
      anim.resize(Rect::new(0, 0, 0, 0));
      anim.step();

      let area = Rect::new(0, 0, 80, 24);
      anim.resize(area);
      for _ in 0..100 {
        anim.step();
      }

      let mut buf = Buffer::empty(area);
      anim.render(area, &mut buf);

      let mut painted_cells = 0usize;
      for y in 0..area.height {
        for x in 0..area.width {
          let cell = &buf[(x, y)];
          let symbol = cell.symbol();
          if symbol == " " || symbol.is_empty() {
            continue;
          }
          painted_cells += 1;
          let mut chars = symbol.chars();
          let Some(ch) = chars.next() else {
            panic!("{:?}: painted empty symbol", info.kind);
          };
          assert!(
            chars.next().is_none(),
            "{:?}: cell ({x},{y}) symbol {symbol:?} has more than one codepoint",
            info.kind
          );
          let width = UnicodeWidthChar::width(ch).unwrap_or(0);
          assert_eq!(
            width, 1,
            "{:?}: cell ({x},{y}) glyph {ch:?} has width {width} (>1) — \
             double-width glyphs corrupt the per-cell diff",
            info.kind
          );
        }
      }
      assert!(
        painted_cells > 0,
        "{:?}: produced no visible output after 100 frames",
        info.kind
      );

      // Resize must change the rendered shape, not silently reuse the old
      // bounds.
      let new_area = Rect::new(0, 0, 40, 12);
      anim.resize(new_area);
      anim.step();
      let mut buf2 = Buffer::empty(new_area);
      anim.render(new_area, &mut buf2);
      assert_eq!(buf2.area, new_area);
    }
  }
}
