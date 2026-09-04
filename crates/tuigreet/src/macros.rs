use greetd_ipc::Request;

pub trait SafeDebug {
  fn safe_repr(&self) -> String;
}

impl SafeDebug for Request {
  fn safe_repr(&self) -> String {
    match self {
      msg @ &Self::CancelSession => format!("{msg:?}"),
      msg @ &Self::CreateSession { .. } => format!("{msg:?}"),
      &Self::PostAuthMessageResponse { .. } => {
        "PostAuthMessageResponse".to_string()
      },
      msg @ &Self::StartSession { .. } => format!("{msg:?}"),
    }
  }
}

/// Look up a message by ID from the compiled Fluent localization bundle.
///
/// If the message ID is not found (e.g. a new key added to source but not yet
/// added to the `.ftl` file), returns the bare ID itself so the UI remains
/// usable instead of panicking.
macro_rules! fl {
  ($message_id:literal) => {{
    let s = $crate::ui::MESSAGES.get($message_id);
    if s.is_empty() {
      $message_id.to_string()
    } else {
      s
    }
  }};

  ($message_id:literal, $($key:ident = $value:expr),*) => {{
    let mut args = std::collections::HashMap::new();
    $(args.insert(stringify!($key), $value);)*
    let s = $crate::ui::MESSAGES.get_args($message_id, args);
    if s.is_empty() {
      $message_id.to_string()
    } else {
      s
    }
  }};
}
