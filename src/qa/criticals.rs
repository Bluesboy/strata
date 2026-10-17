// SPDX-License-Identifier: MIT

//! GLib log lines at warning level or above, collected between replies.

use std::sync::Mutex;

use gtk::glib;

static PENDING: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Installs the process-wide GLib log writer; it still forwards every line to
/// the default writer so the application log stays complete.
pub(crate) fn capture() {
    glib::log_set_writer_func(|level, fields| {
        if matches!(
            level,
            glib::LogLevel::Error | glib::LogLevel::Critical | glib::LogLevel::Warning
        ) {
            let field = |key: &str| {
                fields
                    .iter()
                    .find(|field| field.key() == key)
                    .and_then(glib::LogField::value_str)
                    .unwrap_or_default()
                    .to_owned()
            };
            let line = format!(
                "{}-{} **: {}",
                field("GLIB_DOMAIN"),
                level_name(level),
                field("MESSAGE")
            );
            if let Ok(mut pending) = PENDING.lock() {
                pending.push(line);
            }
        }
        glib::log_writer_default(level, fields)
    });
}

fn level_name(level: glib::LogLevel) -> &'static str {
    match level {
        glib::LogLevel::Error => "ERROR",
        glib::LogLevel::Critical => "CRITICAL",
        _ => "WARNING",
    }
}

pub(crate) fn take() -> Vec<String> {
    PENDING
        .lock()
        .map(|mut pending| std::mem::take(&mut *pending))
        .unwrap_or_default()
}
