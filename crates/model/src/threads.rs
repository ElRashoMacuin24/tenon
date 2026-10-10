//! Screw thread sizes: the ISO metric coarse series and the basic 60 degree profile.
//!
//! The sizes are the first-choice coarse pitches of ISO 261 (nominal diameter and pitch, in mm):
//! a short list of facts, typed in here, not copied from a dataset. See ATTRIBUTION.md.

/// Nominal diameter and coarse pitch, ISO metric, M1 to M64.
pub const ISO_COARSE: [(f64, f64); 21] = [
    (1.0, 0.25),
    (1.2, 0.25),
    (1.6, 0.35),
    (2.0, 0.4),
    (2.5, 0.45),
    (3.0, 0.5),
    (4.0, 0.7),
    (5.0, 0.8),
    (6.0, 1.0),
    (8.0, 1.25),
    (10.0, 1.5),
    (12.0, 1.75),
    (16.0, 2.0),
    (20.0, 2.5),
    (24.0, 3.0),
    (30.0, 3.5),
    (36.0, 4.0),
    (42.0, 4.5),
    (48.0, 5.0),
    (56.0, 5.5),
    (64.0, 6.0),
];

/// Height of the sharp 60 degree triangle of pitch 1: the thread's flanks are cut from it.
pub const TRIANGLE_HEIGHT: f64 = 0.866_025_403_784_438_6;

/// How deep the basic profile is cut for pitch 1: five eighths of the triangle's height. An
/// internal thread's major diameter is its hole (minor diameter) plus twice this.
pub const DEPTH: f64 = 0.625 * TRIANGLE_HEIGHT;

/// The diameter of the hole a thread of this major diameter and pitch is tapped in (the minor
/// diameter of the basic profile).
pub fn minor_diameter(major: f64, pitch: f64) -> f64 {
    major - 2.0 * DEPTH * pitch
}

/// The ISO coarse size nearest a cylindrical face of `diameter`: for a shaft the face is the
/// thread's major diameter, for a hole its minor diameter. Returns (nominal diameter, pitch).
pub fn iso_coarse(diameter: f64, internal: bool) -> (f64, f64) {
    let off = |(nominal, pitch): &(f64, f64)| (if internal { minor_diameter(*nominal, *pitch) } else { *nominal } - diameter).abs();
    ISO_COARSE.iter().copied().min_by(|a, b| off(a).total_cmp(&off(b))).unwrap_or((diameter, diameter / 6.0))
}

/// The pitch for a face of `diameter` when none is given: the nearest coarse size's.
pub fn default_pitch(diameter: f64, internal: bool) -> f64 {
    iso_coarse(diameter, internal).1
}

/// The nominal (major) diameter of a thread of `pitch` on a cylindrical face of `diameter`.
///
/// A shaft is turned to the nominal size or a little under; a hole is drilled between the
/// thread's minor diameter and the usual tap drill (nominal less one pitch). When the face fits a
/// round size that way (a half millimetre step, a tenth below 3 mm), that size is the nominal
/// one: a 6.8 hole and a 7.9 shaft are both M8. Otherwise the size is worked out from the face.
pub fn nominal(diameter: f64, pitch: f64, internal: bool) -> f64 {
    // The middle of where the face's size puts the nominal diameter.
    let about = if internal { diameter + (1.0 + 2.0 * DEPTH) / 2.0 * pitch } else { diameter };
    let step = if about < 3.0 { 0.1 } else { 0.5 };
    let round = (about / step).round() * step;
    if (round - about).abs() <= 0.3 * pitch {
        round
    } else if internal {
        diameter + 2.0 * DEPTH * pitch
    } else {
        diameter
    }
}

/// "M8x1.25" for a thread of this major diameter and pitch; a size that is no whole or half
/// millimetre keeps two decimals ("M7.35x1").
pub fn designation(major: f64, pitch: f64) -> String {
    format!("M{}x{}", trim((major * 100.0).round() / 100.0), trim((pitch * 1000.0).round() / 1000.0))
}

fn trim(x: f64) -> String {
    let s = format!("{x:.3}");
    s.trim_end_matches('0').trim_end_matches('.').to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_follow_the_face_they_are_cut_on() {
        // A shaft is its thread's major diameter; a hole is the tap drill size.
        assert_eq!(iso_coarse(8.0, false), (8.0, 1.25));
        assert_eq!(iso_coarse(8.1, false), (8.0, 1.25));
        assert_eq!(iso_coarse(6.8, true), (8.0, 1.25), "M8 is tapped in a 6.65 hole (6.8 drill)");
        assert_eq!(iso_coarse(5.0, true), (6.0, 1.0));
        assert_eq!(iso_coarse(500.0, false), (64.0, 6.0), "the largest in the list");
        assert!((minor_diameter(8.0, 1.25) - 6.647).abs() < 5e-4);
        assert!((minor_diameter(10.0, 1.5) - 8.376).abs() < 5e-4);
        // The nominal size: the round one the face fits, else worked out from the face.
        assert_eq!(nominal(8.0, 1.25, false), 8.0);
        assert_eq!(nominal(7.9, 1.25, false), 8.0, "a shaft turned a little under size");
        assert_eq!(nominal(6.8, 1.25, true), 8.0, "the usual tap drill");
        assert_eq!(nominal(6.647, 1.25, true), 8.0, "the minor diameter itself");
        assert_eq!(nominal(7.0, 1.0, true), 8.0, "a fine thread");
        assert!((nominal(2.05, 0.45, true) - 2.5).abs() < 1e-12, "tenths below 3 mm");
        assert!((nominal(7.3, 0.5, false) - 7.3).abs() < 1e-12, "no round size near: the shaft's own");
        assert_eq!(nominal(6.0, 0.5, true), 6.5, "a 6 mm hole is the tap drill for M6.5x0.5");
        assert!((nominal(6.2, 0.5, true) - (6.2 + 2.0 * DEPTH * 0.5)).abs() < 1e-12, "no round size near: the hole plus the thread both sides");
        assert_eq!(designation(8.0, 1.25), "M8x1.25");
        assert_eq!(designation(10.0, 1.5), "M10x1.5");
        assert_eq!(designation(7.353, 1.0), "M7.35x1");
        // The list is in order and every pitch is finer than its diameter.
        assert!(ISO_COARSE.windows(2).all(|w| w[0].0 < w[1].0 && w[0].1 <= w[1].1));
        assert!(ISO_COARSE.iter().all(|(d, p)| *p < *d / 3.0));
    }
}
