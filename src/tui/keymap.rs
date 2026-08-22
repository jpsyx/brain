//! Pure key-decision helpers + PTY byte encoding (no `App` mutation).

use crate::tasks::view::View;
use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::layout::{Position, Rect};

use super::model::Panel;

/// Which panel a mouse coordinate falls in. When a brain panel is open it
/// owns the right half (`brain_rect`); a click/scroll inside it routes to
/// `Panel::Brain`. Everything else — including the whole screen when no
/// brain panel is open (`brain_rect` is `None`) — routes to `Panel::Tasks`.
pub(crate) fn panel_at(brain_rect: Option<Rect>, col: u16, row: u16) -> Panel {
    match brain_rect {
        Some(r) if r.contains(Position { x: col, y: row }) => Panel::Brain,
        _ => Panel::Tasks,
    }
}

/// Whether an Enter keystroke in the brain-input modal should insert a
/// newline (multiline compose) rather than submit. We bind this to
/// `Alt+Enter` only: a bare terminal can't distinguish `Shift+Enter` from
/// `Enter` (both are `0x0D`) without the kitty keyboard protocol, whereas
/// `Alt+Enter` arrives as a distinct Meta sequence (`ESC 0x0D`) on every
/// terminal, so it's the one reliable newline binding.
pub(crate) const fn enter_inserts_newline(alt: bool) -> bool {
    alt
}

/// Translate a crossterm `KeyEvent` to the byte sequence a typical
/// xterm-256color terminal would emit. Sequences cover what Claude Code's
/// readline-style prompt actually uses: chars, control codes, arrows,
/// Home/End, PgUp/PgDn, Tab, BackTab, Backspace, Enter, Esc.
pub(crate) fn key_to_bytes(k: &crossterm::event::KeyEvent) -> Option<Vec<u8>> {
    let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
    let alt = k.modifiers.contains(KeyModifiers::ALT);
    let bytes = match k.code {
        KeyCode::Char(c) => {
            if ctrl {
                // Ctrl+A..Ctrl+Z (and a few common symbols) collapse to
                // 0x01..0x1A. Anything else, send literal.
                let lower = c.to_ascii_lowercase();
                if lower.is_ascii_lowercase() {
                    vec![(lower as u8) - b'a' + 1]
                } else if c == ' ' {
                    vec![0]
                } else {
                    let mut buf = [0u8; 4];
                    c.encode_utf8(&mut buf).as_bytes().to_vec()
                }
            } else if alt {
                // ESC + char is the conventional Meta encoding.
                let mut buf = [0u8; 4];
                let s = c.encode_utf8(&mut buf);
                let mut out = Vec::with_capacity(s.len() + 1);
                out.push(0x1B);
                out.extend_from_slice(s.as_bytes());
                out
            } else {
                let mut buf = [0u8; 4];
                c.encode_utf8(&mut buf).as_bytes().to_vec()
            }
        }
        KeyCode::Enter => vec![b'\r'],
        KeyCode::Tab => vec![b'\t'],
        KeyCode::BackTab => vec![0x1B, b'[', b'Z'],
        // 0x7F (DEL) is what xterm sends for Backspace by default and what
        // most modern readline-based programs (including Claude Code)
        // expect. Some legacy programs want 0x08 — we trade that off.
        KeyCode::Backspace => vec![0x7F],
        KeyCode::Esc => vec![0x1B],
        KeyCode::Left => vec![0x1B, b'[', b'D'],
        KeyCode::Right => vec![0x1B, b'[', b'C'],
        KeyCode::Up => vec![0x1B, b'[', b'A'],
        KeyCode::Down => vec![0x1B, b'[', b'B'],
        KeyCode::Home => vec![0x1B, b'[', b'H'],
        KeyCode::End => vec![0x1B, b'[', b'F'],
        KeyCode::PageUp => vec![0x1B, b'[', b'5', b'~'],
        KeyCode::PageDown => vec![0x1B, b'[', b'6', b'~'],
        KeyCode::Delete => vec![0x1B, b'[', b'3', b'~'],
        KeyCode::Insert => vec![0x1B, b'[', b'2', b'~'],
        KeyCode::F(n) => match n {
            1 => vec![0x1B, b'O', b'P'],
            2 => vec![0x1B, b'O', b'Q'],
            3 => vec![0x1B, b'O', b'R'],
            4 => vec![0x1B, b'O', b'S'],
            5 => vec![0x1B, b'[', b'1', b'5', b'~'],
            6 => vec![0x1B, b'[', b'1', b'7', b'~'],
            7 => vec![0x1B, b'[', b'1', b'8', b'~'],
            8 => vec![0x1B, b'[', b'1', b'9', b'~'],
            9 => vec![0x1B, b'[', b'2', b'0', b'~'],
            10 => vec![0x1B, b'[', b'2', b'1', b'~'],
            11 => vec![0x1B, b'[', b'2', b'3', b'~'],
            12 => vec![0x1B, b'[', b'2', b'4', b'~'],
            _ => return None,
        },
        _ => return None,
    };
    Some(bytes)
}

/// In search mode, a Ctrl-modified chord isn't text input — it's a
/// shortcut. Forward it to the normal-mode handler so users can fire
/// task-specific actions (e.g. Ctrl+D → mark complete, Ctrl+Enter → open
/// the task actions modal) without first exiting `/`. Ctrl+Enter matters
/// here because bare Enter exits search instead of opening actions. The
/// palette chord (Ctrl+P) is intercepted upstream in the
/// event loop, so it opens regardless. The two exceptions kept here are
/// Ctrl+C (quit) and Ctrl+U (clear query), which retain their
/// readline-style search-specific bindings.
pub(crate) fn search_delegates_ctrl_chord(code: KeyCode, ctrl: bool) -> bool {
    ctrl && !matches!(code, KeyCode::Char('c' | 'u'))
}

/// In search mode, the chords that abandon the active filter: leave `/`
/// and clear the query. Esc has always done this; Ctrl+C now does too. It
/// used to quit the whole shell, which surprised users who hit it to back
/// out of a search rather than exit the app.
pub(crate) fn search_key_abandons_filter(code: KeyCode, ctrl: bool) -> bool {
    matches!(code, KeyCode::Esc) || (ctrl && matches!(code, KeyCode::Char('c')))
}

/// In search mode, the query-editing chords that double as an exit when
/// there's nothing left to edit: pressing them on an empty query leaves `/`
/// instead of being a no-op. Backspace has always done this; Ctrl+U now
/// mirrors it so a second Ctrl+U (after clearing) backs out of search.
pub(crate) fn search_edit_key_exits_when_empty(code: KeyCode, ctrl: bool) -> bool {
    matches!(code, KeyCode::Backspace) || (ctrl && matches!(code, KeyCode::Char('u')))
}

/// Upper bound on a vim-style count prefix. The list is clamped anyway,
/// but capping keeps the displayed prefix sane and avoids absurd values.
pub(crate) const MAX_COUNT: usize = 9999;

/// Fold a typed digit into a vim-style count prefix. `current` is the
/// count accumulated so far (`None` when no prefix is in progress).
/// Returns the new count, or `None` for a leading `0` (which isn't a
/// count prefix and leaves `0` free for other uses).
pub(crate) fn accumulate_count(current: Option<usize>, digit: u32) -> Option<usize> {
    if current.is_none() && digit == 0 {
        return None;
    }
    let next = current
        .unwrap_or(0)
        .saturating_mul(10)
        .saturating_add(digit as usize)
        .min(MAX_COUNT);
    Some(next)
}

/// Whether a keystroke is one that builds or consumes a vim-style count
/// prefix: a bare digit (accumulates) or a bare `j`/`k`/↓/↑ motion
/// (consumes). Every other key clears any pending count. Ctrl-modified
/// keys never participate.
pub(crate) fn is_count_relevant_key(code: KeyCode, ctrl: bool) -> bool {
    !ctrl
        && matches!(
            code,
            KeyCode::Char('0'..='9' | 'j' | 'k') | KeyCode::Up | KeyCode::Down
        )
}

/// Bare `h` is the habits-view shortcut everywhere except one case: when
/// the highlighted entry already has its notes expanded, `h` collapses
/// them instead (mirroring `l`). This is the only exception where `h`
/// does not switch to the habits view.
pub(crate) fn h_collapses_notes(has_notes: bool, notes_expanded: bool) -> bool {
    has_notes && notes_expanded
}

/// The view a bare single-letter shortcut jumps to, if any. Ctrl-modified
/// keys never switch views — that keeps `Ctrl+P` (and friends) free for the
/// command palette and other chords instead of colliding with the bare
/// `p` = past-due shortcut. `h` is intentionally absent: it doubles as a
/// notes-collapse key, so it's handled inline with that extra context.
/// `b` = backlog view; full-page-up navigation moved to PgUp only when
/// `b` was reclaimed as a view jump.
pub(crate) fn view_shortcut(code: KeyCode, ctrl: bool) -> Option<View> {
    if ctrl {
        return None;
    }
    match code {
        KeyCode::Char('t') => Some(View::Today),
        KeyCode::Char('m') => Some(View::Mit),
        KeyCode::Char('p') => Some(View::PastDue),
        KeyCode::Char('w') => Some(View::Week),
        KeyCode::Char('b') => Some(View::Backlog),
        KeyCode::Char('a') => Some(View::All),
        _ => None,
    }
}

/// Whether a Ctrl-modified keystroke opens the global command palette.
/// Bound to `Ctrl+P`; `Ctrl+K` was retired so it stays available as a
/// palette-internal up-navigation alias.
pub(crate) fn ctrl_opens_palette(code: KeyCode) -> bool {
    matches!(code, KeyCode::Char('p' | 'P'))
}

/// Whether a keystroke is the unconditional "quit the whole shell" chord.
/// Bound to `Ctrl+Q` (`0x11`, so no kitty-protocol dependency). Intercepted
/// before modal routing and panel dispatch so nothing can swallow it: it
/// quits from either panel and even while a modal is open — unlike bare `q` /
/// `Ctrl+C`, which stay contextual (dismiss modals, quit only from the tasks
/// panel's normal mode, forwarded to claude in the brain panel).
pub(crate) fn ctrl_quits(code: KeyCode, ctrl: bool) -> bool {
    ctrl && matches!(code, KeyCode::Char('q' | 'Q'))
}

/// Whether a keystroke scrolls the focused panel by a half page. Normally this
/// is `Alt+U` / `Alt+D`. In richer keyboard modes on macOS, Option can surface
/// as the produced glyph instead of an Alt-modified ASCII key, so accept those
/// equivalent glyphs too.
#[must_use]
pub(crate) fn alt_scroll_direction(code: KeyCode, modifiers: KeyModifiers) -> Option<bool> {
    let modified = modifiers.intersects(KeyModifiers::ALT | KeyModifiers::META);
    match code {
        KeyCode::Char('u' | 'U') if modified => Some(true),
        KeyCode::Char('d' | 'D') if modified => Some(false),
        KeyCode::Char('\u{00a8}' | '\u{0308}') => Some(true),
        KeyCode::Char('\u{2202}') => Some(false),
        _ => None,
    }
}

/// A brain-panel tab a keystroke asks for: which slot, and whether it arrived as
/// a deliberate `Alt`-modified chord.
///
/// The distinction decides what happens when the slot holds no tab. A chord is a
/// tab request that simply missed, so the shell swallows it. A bare
/// Option-produced glyph is *also* ordinary text (`£50`), so it must reach the
/// panel rather than vanish.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct BrainTabSlot {
    pub(crate) index: usize,
    pub(crate) from_chord: bool,
}

/// Which brain-panel tab *slot* a keystroke selects, if any: `0` for `Alt+1` (the
/// main session) and `n` for `Alt+<n+1>` (the nth open skill session). The caller
/// resolves a slot against the tabs actually open, so an unoccupied digit selects
/// nothing. We use `Alt+digit` rather than `Ctrl+digit` because most terminals
/// don't distinguish `Ctrl+1` from a bare `1` (only the kitty keyboard protocol
/// does), whereas `Alt+digit` arrives as a distinct Meta sequence everywhere. On
/// macOS layouts where Option surfaces the produced glyph instead of an
/// Alt-modified ASCII digit, accept the `Option+<digit>` glyphs too — flagged as
/// not-a-chord, since those glyphs are typeable characters in their own right.
#[must_use]
pub(crate) fn alt_selects_brain_tab_slot(
    code: KeyCode,
    modifiers: KeyModifiers,
) -> Option<BrainTabSlot> {
    let modified = modifiers.intersects(KeyModifiers::ALT | KeyModifiers::META);
    match code {
        KeyCode::Char(digit @ '1'..='9') if modified => Some(BrainTabSlot {
            index: digit.to_digit(10)? as usize - 1,
            from_chord: true,
        }),
        // macOS US-layout Option+1..Option+9 glyphs, in digit order.
        KeyCode::Char(glyph) => MAC_OPTION_DIGIT_GLYPHS
            .iter()
            .position(|&candidate| candidate == glyph)
            .map(|index| BrainTabSlot {
                index,
                from_chord: false,
            }),
        _ => None,
    }
}

/// The glyphs a macOS US layout produces for `Option+1` … `Option+9`, in digit
/// order, for terminals that send the glyph instead of an Alt-modified digit.
const MAC_OPTION_DIGIT_GLYPHS: [char; 9] = [
    '\u{00a1}', // ¡
    '\u{2122}', // ™
    '\u{00a3}', // £
    '\u{00a2}', // ¢
    '\u{221e}', // ∞
    '\u{00a7}', // §
    '\u{00b6}', // ¶
    '\u{2022}', // •
    '\u{00aa}', // ª
];

/// Whether a keystroke cycles the brain-panel tab, and in which direction:
/// `Some(true)` = next (`Alt+]`), `Some(false)` = previous (`Alt+[`). This is
/// the *reliable* tab switch: unlike `Alt+digit` (which many terminals can't
/// distinguish from a bare digit), the bracket keys resolve either as an
/// Alt-modified `[` / `]` or, on macOS US layouts with Option-as-glyph, as the
/// Option-produced smart-quote glyphs — both of which we accept. The order is
/// the tab strip's: the main session, then each open skill session.
#[must_use]
pub(crate) fn alt_cycles_brain_tab(code: KeyCode, modifiers: KeyModifiers) -> Option<bool> {
    let modified = modifiers.intersects(KeyModifiers::ALT | KeyModifiers::META);
    match code {
        KeyCode::Char(']') if modified => Some(true),
        KeyCode::Char('[') if modified => Some(false),
        // macOS US-layout Option+] (‘ U+2018) / Option+[ (“ U+201C).
        KeyCode::Char('\u{2018}') => Some(true),
        KeyCode::Char('\u{201C}') => Some(false),
        _ => None,
    }
}

/// Whether a keystroke fires the "open links" action (Linear issue +
/// notes URLs). Bound to `Ctrl+O`; a single link opens directly, multiple
/// links raise the picker modal.
pub(crate) fn ctrl_opens_links(code: KeyCode, ctrl: bool) -> bool {
    ctrl && matches!(code, KeyCode::Char('o' | 'O'))
}

/// Whether a keystroke opens (or focuses) the persistent brain panel. Bound
/// to `Ctrl+M` *without* Shift. Many terminals encode Ctrl+M identically to
/// `Enter` (both → 0x0D), so this only fires distinctly under the kitty
/// keyboard protocol; without it, Ctrl+M collapses to `Enter`. The `!shift`
/// guard keeps it distinct from its Shift-modified sibling
/// [`ctrl_messages_brain_about_task`]. We match both `'m'` and `'M'` because
/// terminals differ on whether they report the shifted glyph.
pub(crate) fn ctrl_opens_brain(code: KeyCode, ctrl: bool, shift: bool) -> bool {
    ctrl && !shift && matches!(code, KeyCode::Char('m' | 'M'))
}

/// Whether a keystroke fires the task-scoped "message brain about this task"
/// action. Bound to `Ctrl+Shift+M` — the Shift-modified sibling of
/// [`ctrl_opens_brain`]. Telling the two apart requires the kitty keyboard
/// protocol's modifier reporting; on terminals without it, Ctrl+Shift+M
/// collapses to `Enter` and the palette / task-actions modal is the fallback.
pub(crate) fn ctrl_messages_brain_about_task(code: KeyCode, ctrl: bool, shift: bool) -> bool {
    ctrl && shift && matches!(code, KeyCode::Char('m' | 'M'))
}

/// Whether a keystroke fires the destructive "remove task" shortcut. Bound
/// to `Ctrl+Backspace`: bare Backspace is too easy to hit by accident, so
/// removal requires the modifier. (Reachable on terminals that report the
/// modifier via the kitty protocol; the palette / task-actions modal is the
/// always-available fallback.)
pub(crate) fn ctrl_removes_task(code: KeyCode, ctrl: bool) -> bool {
    ctrl && matches!(code, KeyCode::Backspace)
}
