//! Pure functions that turn `Task`s into styled `ratatui` `Line`s.
//!
//! Design notes:
//! - Tokyo-Night-inspired palette declared as `const` (`name-consts-screaming`).
//! - No background-colored badges — colored text only. The visual hierarchy is
//!   carried by bold-weight on task names and a continuous priority-colored
//!   accent gutter on every line of a card. Blank line between cards = the
//!   gutter's absence becomes the separator.
//! - Each task renders to 3–5 short lines via small per-line builders.
//! - Public helpers are `#[must_use]`; pattern→Color/Style lookups are
//!   `const fn` so the compiler can inline them.
//!
//! Module layout:
//! - [`style`] — palette + style/label primitives + inline span builders.
//! - [`markdown`] — the inline + block markdown subset (task notes).
//! - [`card`] — per-task card composition and the body builder.
//! - [`chrome`] — header banner, footers, search bar, empty state.

mod card;
mod chrome;
mod markdown;
mod style;

pub use card::build_body_lines_with_ranges;
pub(crate) use chrome::{
    compact_footer_line, header_lines, no_matches_lines, search_bar_line, search_footer_line,
};
pub use style::{status_label, truncate, type_label};
