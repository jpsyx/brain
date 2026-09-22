//! sRGB ↔ Oklab, and the one operation the tree's cursor needs.
//!
//! The matrices and the transfer functions are Björn Ottosson's reference
//! values (`bottosson.github.io/posts/oklab/`), kept digit-for-digit so they
//! can be checked against the source; the underscores are grouping only. The
//! arithmetic is `f64` because those constants carry more precision than an
//! `f32` literal can hold, and the conversion is exact to the byte either way.

use ratatui::style::Color;

// ---------------------------------------------------------------------------
// Ottosson's matrices
// ---------------------------------------------------------------------------

/// Linear sRGB → the long-wavelength cone response.
const LINEAR_TO_LONG: [f64; 3] = [0.412_221_470_8, 0.536_332_536_3, 0.051_445_992_9];
/// Linear sRGB → the medium-wavelength cone response.
const LINEAR_TO_MED: [f64; 3] = [0.211_903_498_2, 0.680_699_545_1, 0.107_396_956_6];
/// Linear sRGB → the short-wavelength cone response.
const LINEAR_TO_SHORT: [f64; 3] = [0.088_302_461_9, 0.281_718_837_6, 0.629_978_700_5];

/// Cube-rooted cone responses → Oklab lightness.
const CONES_TO_LIGHTNESS: [f64; 3] = [0.210_454_255_3, 0.793_617_785_0, -0.004_072_046_8];
/// Cube-rooted cone responses → the green-to-red chroma axis.
const CONES_TO_GREEN_RED: [f64; 3] = [1.977_998_495_1, -2.428_592_205_0, 0.450_593_709_9];
/// Cube-rooted cone responses → the blue-to-yellow chroma axis.
const CONES_TO_BLUE_YELLOW: [f64; 3] = [0.025_904_037_1, 0.782_771_766_2, -0.808_675_766_0];

/// Oklab → the long-wavelength cone response, before cubing.
const OKLAB_TO_LONG: [f64; 3] = [1.0, 0.396_337_777_4, 0.215_803_757_3];
/// Oklab → the medium-wavelength cone response, before cubing.
const OKLAB_TO_MED: [f64; 3] = [1.0, -0.105_561_345_8, -0.063_854_172_8];
/// Oklab → the short-wavelength cone response, before cubing.
const OKLAB_TO_SHORT: [f64; 3] = [1.0, -0.089_484_177_5, -1.291_485_548_0];

/// Cone responses → linear sRGB red.
const CONES_TO_RED: [f64; 3] = [4.076_741_662_1, -3.307_711_591_3, 0.230_969_929_2];
/// Cone responses → linear sRGB green.
const CONES_TO_GREEN: [f64; 3] = [-1.268_438_004_6, 2.609_757_401_1, -0.341_319_396_5];
/// Cone responses → linear sRGB blue.
const CONES_TO_BLUE: [f64; 3] = [-0.004_196_086_3, -0.703_418_614_7, 1.707_614_701_0];

// ---------------------------------------------------------------------------
// The model
// ---------------------------------------------------------------------------

/// A colour in Oklab: perceptual lightness plus the two chroma axes.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Oklab {
    pub(crate) lightness: f64,
    /// Ottosson's `a`: the green-to-red chroma axis.
    pub(crate) green_red: f64,
    /// Ottosson's `b`: the blue-to-yellow chroma axis.
    pub(crate) blue_yellow: f64,
}

/// Return `color` with its Oklab lightness raised by `amount` (0.15 = 15%).
///
/// Oklab is perceptually uniform, so scaling `L` brightens without the hue
/// drift that scaling sRGB channels produces. Only `Color::Rgb` has a known
/// RGB to lift: a named colour, an `Indexed` one, or `Reset` comes back
/// untouched rather than guessed at. A lift that would leave the sRGB gamut
/// clamps per channel, which spends chroma on the lightness it was asked for.
#[must_use]
pub(crate) fn lighten(color: Color, amount: f32) -> Color {
    let Some(lab) = to_oklab(color) else {
        return color;
    };
    to_color(Oklab {
        lightness: (lab.lightness * (1.0 + f64::from(amount))).min(1.0),
        ..lab
    })
}

/// The Oklab coordinates of `color`, or `None` when it names no RGB.
#[must_use]
pub(crate) fn to_oklab(color: Color) -> Option<Oklab> {
    let Color::Rgb(red, green, blue) = color else {
        return None;
    };
    let linear = [
        decode_gamma(f64::from(red) / 255.0),
        decode_gamma(f64::from(green) / 255.0),
        decode_gamma(f64::from(blue) / 255.0),
    ];
    let cones = [
        dot(LINEAR_TO_LONG, linear).cbrt(),
        dot(LINEAR_TO_MED, linear).cbrt(),
        dot(LINEAR_TO_SHORT, linear).cbrt(),
    ];
    Some(Oklab {
        lightness: dot(CONES_TO_LIGHTNESS, cones),
        green_red: dot(CONES_TO_GREEN_RED, cones),
        blue_yellow: dot(CONES_TO_BLUE_YELLOW, cones),
    })
}

/// The `Color::Rgb` nearest `lab`, clamped into the sRGB gamut.
#[must_use]
pub(crate) fn to_color(lab: Oklab) -> Color {
    let axes = [lab.lightness, lab.green_red, lab.blue_yellow];
    let cones = [
        cube(dot(OKLAB_TO_LONG, axes)),
        cube(dot(OKLAB_TO_MED, axes)),
        cube(dot(OKLAB_TO_SHORT, axes)),
    ];
    Color::Rgb(
        to_byte(dot(CONES_TO_RED, cones)),
        to_byte(dot(CONES_TO_GREEN, cones)),
        to_byte(dot(CONES_TO_BLUE, cones)),
    )
}

// ---------------------------------------------------------------------------
// Arithmetic
// ---------------------------------------------------------------------------

/// One conversion-matrix row applied to a colour vector.
///
/// Spelled with `mul_add` rather than as a plain sum of products so the whole
/// matrix keeps one shape and the rounding is the same in every row.
fn dot(row: [f64; 3], value: [f64; 3]) -> f64 {
    row[0].mul_add(value[0], row[1].mul_add(value[1], row[2] * value[2]))
}

fn cube(value: f64) -> f64 {
    value * value * value
}

/// sRGB's transfer function, inverted: a stored channel to light intensity.
fn decode_gamma(channel: f64) -> f64 {
    if channel <= 0.040_45 {
        channel / 12.92
    } else {
        ((channel + 0.055) / 1.055).powf(2.4)
    }
}

/// sRGB's transfer function: light intensity back to a stored channel.
fn encode_gamma(intensity: f64) -> f64 {
    if intensity <= 0.003_130_8 {
        12.92 * intensity
    } else {
        1.055_f64.mul_add(intensity.powf(1.0 / 2.4), -0.055)
    }
}

/// A linear intensity as the byte sRGB stores it.
///
/// Clamping before the cast is what makes the cast exact, and is also the
/// gamut decision: a colour lifted past what sRGB can show lands on the
/// nearest colour it can, rather than wrapping to a wildly different one.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "clamped to 0.0..=255.0 and rounded, so the value is exactly a u8"
)]
fn to_byte(intensity: f64) -> u8 {
    (encode_gamma(intensity) * 255.0).clamp(0.0, 255.0).round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::{
        ACCENT_CYAN, ACCENT_GREEN, ACCENT_PURPLE, ACCENT_RED, ACCENT_YELLOW, MATCH_HIGHLIGHT,
        SELECTED_BG, TEXT_DIM, TEXT_PRIMARY, TEXT_VERY_DIM,
    };

    /// Every colour the palette actually paints with, so a retune cannot slip
    /// a colour past the round-trip and lift guarantees.
    const PALETTE: [(&str, Color); 10] = [
        ("TEXT_PRIMARY", TEXT_PRIMARY),
        ("TEXT_DIM", TEXT_DIM),
        ("TEXT_VERY_DIM", TEXT_VERY_DIM),
        ("ACCENT_PURPLE", ACCENT_PURPLE),
        ("ACCENT_CYAN", ACCENT_CYAN),
        ("ACCENT_YELLOW", ACCENT_YELLOW),
        ("ACCENT_GREEN", ACCENT_GREEN),
        ("ACCENT_RED", ACCENT_RED),
        ("MATCH_HIGHLIGHT", MATCH_HIGHLIGHT),
        ("SELECTED_BG", SELECTED_BG),
    ];

    const LIFT: f32 = 0.18;

    fn bytes_of(color: Color) -> (u8, u8, u8) {
        match color {
            Color::Rgb(red, green, blue) => (red, green, blue),
            other => panic!("{other:?} is not an RGB colour"),
        }
    }

    fn lightness_of(color: Color) -> f64 {
        to_oklab(color).expect("a palette colour is RGB").lightness
    }

    /// The Oklab chroma direction: the angle the hue sits at, in radians.
    fn hue_of(color: Color) -> f64 {
        let lab = to_oklab(color).expect("a palette colour is RGB");
        lab.blue_yellow.atan2(lab.green_red)
    }

    /// Whether the lift ran out of sRGB: a channel pinned at either end had
    /// to stop short of the colour Oklab asked for, which costs chroma and so
    /// moves the hue. Nothing inside the gamut is allowed that excuse.
    fn hit_the_gamut_wall(color: Color) -> bool {
        let (red, green, blue) = bytes_of(color);
        let pinned = |channel: u8| channel == 0 || channel == u8::MAX;
        pinned(red) || pinned(green) || pinned(blue)
    }

    fn within(actual: f64, expected: f64, tolerance: f64) -> bool {
        (actual - expected).abs() <= tolerance
    }

    #[test]
    fn every_palette_colour_survives_a_round_trip_through_oklab() {
        for (name, color) in PALETTE {
            let lab = to_oklab(color).expect("a palette colour is RGB");
            let (red, green, blue) = bytes_of(color);
            let (back_red, back_green, back_blue) = bytes_of(to_color(lab));

            for (channel, original, returned) in [
                ("red", red, back_red),
                ("green", green, back_green),
                ("blue", blue, back_blue),
            ] {
                assert!(
                    original.abs_diff(returned) <= 1,
                    "{name} {channel}: {original} came back as {returned}"
                );
            }
        }
    }

    #[test]
    fn lightening_raises_the_perceived_lightness_of_every_palette_colour() {
        for (name, color) in PALETTE {
            let before = lightness_of(color);
            let after = lightness_of(lighten(color, LIFT));

            assert!(after > before, "{name}: lightness went {before} -> {after}");
        }
    }

    #[test]
    fn the_lift_is_a_proportional_step_in_perceptual_lightness() {
        // The contract, and the reason this is done in Oklab rather than by
        // multiplying sRGB bytes: `amount` is a fraction of *perceived*
        // lightness, so the same amount reads as the same amount of
        // brightening on a near-black row and on a near-white one. The same
        // ×1.18 applied to sRGB bytes lands between 1.10 and 1.13 here, and
        // by a different factor for each colour.
        let in_gamut: Vec<(&str, Color)> = PALETTE
            .into_iter()
            .filter(|(_, color)| !hit_the_gamut_wall(lighten(*color, LIFT)))
            .collect();
        assert!(
            in_gamut.len() >= 3,
            "the palette must keep colours the lift can serve exactly: {in_gamut:?}"
        );

        for (name, color) in in_gamut {
            let ratio = lightness_of(lighten(color, LIFT)) / lightness_of(color);

            assert!(
                within(ratio, f64::from(1.0 + LIFT), 0.008),
                "{name}: lightness scaled by {ratio}, not {}",
                1.0 + LIFT
            );
        }
    }

    #[test]
    fn lightening_leaves_the_hue_where_it_was() {
        // `lighten` touches only `L`, so a colour the sRGB gamut can still
        // hold comes back the same hue: what is left is byte rounding.
        for (name, color) in PALETTE {
            let lifted = lighten(color, LIFT);
            if hit_the_gamut_wall(lifted) {
                continue;
            }

            assert!(
                within(hue_of(lifted), hue_of(color), 0.03),
                "{name}: hue moved {} -> {}",
                hue_of(color),
                hue_of(lifted)
            );
        }
    }

    #[test]
    fn a_saturated_colour_keeps_its_hue_almost_exactly() {
        // The tightest case the palette offers: green is the one saturated
        // accent an 18% lift does not push out of the sRGB gamut, so nothing
        // but rounding can move it. (Red is *more* saturated but clips —
        // 247 wants to become 294 — and clipping is what moves a hue.)
        let lifted = lighten(ACCENT_GREEN, LIFT);

        assert!(!hit_the_gamut_wall(lifted), "{lifted:?} clipped");
        assert!(
            within(hue_of(lifted), hue_of(ACCENT_GREEN), 0.01),
            "hue moved {} -> {}",
            hue_of(ACCENT_GREEN),
            hue_of(lifted)
        );
    }

    #[test]
    fn a_colour_with_no_known_rgb_is_handed_back_untouched() {
        // Guessing at an RGB for these would repaint a terminal's own palette
        // choice, which is the user's, not ours.
        assert_eq!(lighten(Color::Reset, 0.15), Color::Reset);
        assert_eq!(lighten(Color::Cyan, 0.15), Color::Cyan);
        assert_eq!(lighten(Color::Indexed(42), 0.15), Color::Indexed(42));
    }

    #[test]
    fn white_saturates_rather_than_overshooting() {
        let white = Color::Rgb(u8::MAX, u8::MAX, u8::MAX);

        assert_eq!(lighten(white, 0.15), white);
    }

    #[test]
    fn a_lift_of_nothing_is_the_colour_itself() {
        for (name, color) in PALETTE {
            let (red, green, blue) = bytes_of(color);
            let (lit_red, lit_green, lit_blue) = bytes_of(lighten(color, 0.0));

            for (channel, original, returned) in [
                ("red", red, lit_red),
                ("green", green, lit_green),
                ("blue", blue, lit_blue),
            ] {
                assert!(
                    original.abs_diff(returned) <= 1,
                    "{name} {channel}: {original} came back as {returned}"
                );
            }
        }
    }
}
