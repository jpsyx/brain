//! Terminal color theme for brain's **non-TUI CLI** output (sync, setup, config,
//! env, personalize, doctor).
//!
//! Callers ask for MEANING, not color: semantic design tokens (`heading`,
//! `accent`, `value`, `muted`, `success`, `warning`, `error`, `info`, `prompt`)
//! each map to an ANSI SGR code. Colors are chosen for **dark terminals** (the
//! assumed default — see AGENTS.md "Aesthetics"): bright, high-contrast variants,
//! never a dark foreground on a dark background. The [`Theme`] struct makes adding
//! a light (or other) theme a matter of swapping the code table; [`Theme::active`]
//! picks the theme today (always dark: terminals don't reliably expose a
//! light/dark token) and bakes in whether color is emitted at all.

use std::io::IsTerminal;

/// A palette: one ANSI SGR **body** per semantic role (e.g. `"96"` = bright
/// cyan), plus whether color is emitted. [`Theme::paint`] wraps a body with the
/// CSI prefix and reset.
#[derive(Debug, Clone, Copy)]
pub struct Theme {
    color: bool,
    heading_code: &'static str,
    accent_code: &'static str,
    value_code: &'static str,
    muted_code: &'static str,
    success_code: &'static str,
    warning_code: &'static str,
    error_code: &'static str,
    info_code: &'static str,
    prompt_code: &'static str,
}

impl Theme {
    /// The dark-terminal theme (brain's default). `color` gates emission so the
    /// same theme yields plain text when piped / `NO_COLOR` / not a TTY.
    #[must_use]
    pub const fn dark(color: bool) -> Self {
        Self {
            color,
            heading_code: "1;95",  // bold bright magenta
            accent_code: "96",     // bright cyan — keys, commands, labels
            value_code: "97",      // bright white — values / emphasis
            muted_code: "90",      // gray — hints, secondary text
            success_code: "92",    // bright green
            warning_code: "93",    // bright yellow
            error_code: "91",      // bright red
            info_code: "94",       // bright blue (never plain "34" — too dark on dark)
            prompt_code: "1;96",   // bold bright cyan — interactive prompts
        }
    }

    /// The active theme with color auto-detected. Dark for now; a future light
    /// theme would be chosen here.
    #[must_use]
    pub fn active() -> Self {
        Self::dark(color_enabled())
    }

    fn paint(self, code: &str, s: &str) -> String {
        if self.color && !code.is_empty() {
            format!("\x1b[{code}m{s}\x1b[0m")
        } else {
            s.to_owned()
        }
    }

    #[must_use]
    pub fn heading(self, s: &str) -> String {
        self.paint(self.heading_code, s)
    }
    #[must_use]
    pub fn accent(self, s: &str) -> String {
        self.paint(self.accent_code, s)
    }
    #[must_use]
    pub fn value(self, s: &str) -> String {
        self.paint(self.value_code, s)
    }
    #[must_use]
    pub fn muted(self, s: &str) -> String {
        self.paint(self.muted_code, s)
    }
    #[must_use]
    pub fn success(self, s: &str) -> String {
        self.paint(self.success_code, s)
    }
    #[must_use]
    pub fn warning(self, s: &str) -> String {
        self.paint(self.warning_code, s)
    }
    #[must_use]
    pub fn error(self, s: &str) -> String {
        self.paint(self.error_code, s)
    }
    #[must_use]
    pub fn info(self, s: &str) -> String {
        self.paint(self.info_code, s)
    }
    #[must_use]
    pub fn prompt(self, s: &str) -> String {
        self.paint(self.prompt_code, s)
    }
}

/// Whether to emit ANSI escapes: stderr is a terminal and `NO_COLOR` is unset.
///
/// brain's *stdout* is captured by the `run.sh` wrapper (never a TTY) and
/// reprinted verbatim, so terminal-ness is judged from **stderr**.
#[must_use]
pub fn color_enabled() -> bool {
    std::io::stderr().is_terminal() && std::env::var_os("NO_COLOR").is_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colored_theme_wraps_tokens_in_their_sgr_codes() {
        let t = Theme::dark(true);
        assert_eq!(t.success("ok"), "\x1b[92mok\x1b[0m");
        assert_eq!(t.error("no"), "\x1b[91mno\x1b[0m");
        assert_eq!(t.accent("key"), "\x1b[96mkey\x1b[0m");
        assert_eq!(t.heading("Title"), "\x1b[1;95mTitle\x1b[0m");
    }

    #[test]
    fn uncolored_theme_is_plain_text() {
        let t = Theme::dark(false);
        assert_eq!(t.success("ok"), "ok");
        assert_eq!(t.heading("Title"), "Title");
        assert!(!t.warning("hmm").contains('\x1b'));
    }

    #[test]
    fn dark_theme_avoids_low_contrast_foregrounds() {
        // Designed for dark terminals: every color token is a bright (9x) or
        // bold variant, never a dim 30-37 foreground that would vanish on dark.
        let t = Theme::dark(true);
        for token in [
            t.accent("x"), t.value("x"), t.success("x"), t.warning("x"),
            t.error("x"), t.info("x"), t.prompt("x"), t.heading("x"),
        ] {
            let uses_bright = token.contains("[9") || token.contains(";9");
            assert!(uses_bright, "token should use a bright color: {token:?}");
            // Never a plain dark blue (the example the user called out).
            assert!(!token.contains("[34m"), "must not use dark blue: {token:?}");
        }
    }
}
