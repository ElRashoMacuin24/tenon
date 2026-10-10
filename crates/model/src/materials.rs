//! Materials: what a part is made of, which gives its mass and its colour.
//!
//! The library is a short list of common materials with typical densities, typed in from
//! general engineering knowledge (values a handbook or a supplier's sheet would give to two or
//! three figures); no dataset is copied. A part keeps its material's name, density and colour in
//! its own file, so it weighs and looks the same wherever it is opened.

use serde::{Deserialize, Serialize};

/// What a part is made of.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Material {
    pub name: String,
    /// Grams per cubic centimetre.
    pub density: f64,
    /// Its colour on screen, as "#rrggbb".
    pub color: String,
}

/// The density of a part with no material: water's, so a gram is a cubic centimetre.
pub const DEFAULT_DENSITY: f64 = 1.0;
/// The heaviest density accepted, g/cm^3 (osmium is 22.6).
pub const MAX_DENSITY: f64 = 30.0;

impl Material {
    pub fn check(&self) -> Result<(), String> {
        let n = self.name.trim().chars().count();
        if n == 0 || n > 64 {
            return Err("a material's name must be 1 to 64 characters".into());
        }
        if !(self.density.is_finite() && self.density > 0.0 && self.density <= MAX_DENSITY) {
            return Err(format!("a material's density must be more than 0 and at most {MAX_DENSITY} g/cm^3"));
        }
        parse_color(&self.color).map(|_| ()).ok_or_else(|| format!("`{}` is not a colour: write it as #rrggbb", self.color))
    }
}

/// A material in the library: name, density (g/cm^3) and colour.
pub const LIBRARY: [(&str, f64, &str); 22] = [
    ("Steel, mild", 7.85, "#8a9199"),
    ("Stainless steel", 8.0, "#b4bac1"),
    ("Cast iron", 7.2, "#5f646b"),
    ("Aluminium 6061", 2.7, "#c3c8ce"),
    ("Brass", 8.5, "#c9a64a"),
    ("Bronze", 8.8, "#a9783c"),
    ("Copper", 8.96, "#c47a4e"),
    ("Titanium", 4.43, "#9a9ea6"),
    ("ABS", 1.04, "#e8e4d8"),
    ("PLA", 1.24, "#4f9fd8"),
    ("PETG", 1.27, "#6fc3b8"),
    ("Nylon (PA6)", 1.14, "#e9e6dc"),
    ("Polycarbonate", 1.2, "#cfe1ea"),
    ("Acetal (POM)", 1.41, "#f2f0ea"),
    ("Acrylic (PMMA)", 1.18, "#d6eaf2"),
    ("HDPE", 0.95, "#eeeeea"),
    ("TPU", 1.21, "#3c3f44"),
    ("Resin, standard", 1.18, "#8f8f96"),
    ("Oak", 0.75, "#b08a5a"),
    ("Pine", 0.5, "#dcc08e"),
    ("Plywood, birch", 0.68, "#d2b683"),
    ("MDF", 0.75, "#b99a73"),
];

/// The library's material of this name (capitals do not matter).
pub fn find(name: &str) -> Option<Material> {
    let wanted = name.trim().to_lowercase();
    LIBRARY.iter().find(|m| m.0.to_lowercase() == wanted).map(|(name, density, color)| Material {
        name: (*name).to_owned(),
        density: *density,
        color: (*color).to_owned(),
    })
}

/// "#rrggbb" as red, green and blue.
pub fn parse_color(s: &str) -> Option<[u8; 3]> {
    let hex = s.trim().strip_prefix('#')?;
    if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    Some([byte(0)?, byte(2)?, byte(4)?])
}

/// Red, green and blue as "#rrggbb".
pub fn color_text(c: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_library_holds_materials_a_part_can_take() {
        for (name, ..) in LIBRARY {
            let m = find(name).unwrap();
            m.check().unwrap_or_else(|e| panic!("{name}: {e}"));
        }
        // Found whatever the capitals; names are not repeated.
        assert_eq!(find("  aluminium 6061 ").map(|m| m.density), Some(2.7));
        assert_eq!(find("unobtainium"), None);
        let mut names: Vec<String> = LIBRARY.iter().map(|m| m.0.to_lowercase()).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), LIBRARY.len());
    }

    #[test]
    fn colours_are_written_as_hash_and_six_hex_digits() {
        assert_eq!(parse_color("#c3c8ce"), Some([0xc3, 0xc8, 0xce]));
        assert_eq!(parse_color(" #FFffFF "), Some([255, 255, 255]));
        assert_eq!(color_text([0, 128, 255]), "#0080ff");
        for bad in ["c3c8ce", "#c3c8c", "#c3c8cez", "red", "", "#ggg000"] {
            assert_eq!(parse_color(bad), None, "{bad}");
        }
    }

    #[test]
    fn a_material_must_have_a_name_a_density_and_a_colour() {
        let ok = Material { name: "Mine".into(), density: 2.0, color: "#102030".into() };
        ok.check().unwrap();
        assert!(Material { name: " ".into(), ..ok.clone() }.check().is_err());
        assert!(Material { density: 0.0, ..ok.clone() }.check().unwrap_err().contains("more than 0"));
        assert!(Material { density: 40.0, ..ok.clone() }.check().is_err());
        assert!(Material { density: f64::NAN, ..ok.clone() }.check().is_err());
        assert!(Material { color: "blue".into(), ..ok }.check().unwrap_err().contains("#rrggbb"));
    }
}
