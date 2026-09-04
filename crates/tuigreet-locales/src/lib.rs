use std::{ops::Deref, sync::OnceLock};

use i18n_embed::{
  DesktopLanguageRequester,
  LanguageLoader,
  fluent::{FluentLanguageLoader, fluent_language_loader},
};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "locales"]
struct Localizations;

pub struct LazyLoader {
  once: OnceLock<FluentLanguageLoader>,
}

impl LazyLoader {
  const fn new() -> Self {
    Self {
      once: OnceLock::new(),
    }
  }
}

impl Deref for LazyLoader {
  type Target = FluentLanguageLoader;

  fn deref(&self) -> &Self::Target {
    self.once.get_or_init(|| {
      let locales = Localizations;
      let loader = fluent_language_loader!();

      // Load only the fallback language — the one locale we guarantee is
      // embedded. Never pass system-requested languages to `load_languages`
      // because it hard-errors if any of them are missing from the binary.
      // `i18n_embed::select` below will apply whatever the system asked for
      // on top of this base; if the system's preferred locale isn't
      // embedded the fallback is silently used instead.
      let fallback = loader.fallback_language().clone();
      let fallback_for_display = fallback.clone();
      if let Err(e) = loader.load_languages(&locales, &[fallback]) {
        eprintln!(
          "tuigreet: could not load fallback locale '{}': {}",
          fallback_for_display,
          e
        );
      }

      let _ = i18n_embed::select(
        &loader,
        &locales,
        &DesktopLanguageRequester::requested_languages(),
      );

      loader
    })
  }
}

pub static MESSAGES: LazyLoader = LazyLoader::new();
