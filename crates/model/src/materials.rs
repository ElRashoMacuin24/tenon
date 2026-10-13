//! Materials: what a part is made of, which gives its mass and its colour, and (for a stress
//! check) how it answers to a load.
//!
//! The library is a short list of common materials with typical densities, stiffnesses and
//! strengths, typed in from general engineering knowledge (values a handbook or a supplier's
//! sheet would give to two or three figures); no dataset is copied. They are typical, not
//! guaranteed: what a given bar or filament has is on its supplier's sheet. A part keeps its
//! material's name, density, colour and strength in its own file, so it weighs, looks and
//! checks the same wherever it is opened.

use serde::{Deserialize, Serialize};

/// What a part is made of.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Material {
    pub name: String,
    /// Grams per cubic centimetre.
    pub density: f64,
    /// Its colour on screen, as "#rrggbb".
    pub color: String,
    /// How it answers to a load, where that is known and the same in every direction.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strength: Option<Strength>,
}

/// How a material answers to a load: what a stress check needs of it (M11). For a material
/// that is the same in every direction; wood, whose strength lies along its grain, has none.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Strength {
    /// Young's modulus, GPa: how stiff it is.
    pub youngs_modulus: f64,
    /// Poisson's ratio: how much it thins as it is stretched.
    pub poisson: f64,
    /// The stress at which it stays bent, MPa. For a material that breaks before it bends
    /// (cast iron, acrylic), the stress at which it breaks.
    pub yield_strength: f64,
}

impl Strength {
    pub fn check(&self) -> Result<(), String> {
        if !(self.youngs_modulus.is_finite() && self.youngs_modulus > 0.0 && self.youngs_modulus <= 2000.0) {
            return Err("Young's modulus must be more than 0 and at most 2000 GPa".into());
        }
        if !(self.poisson.is_finite() && (0.0..0.5).contains(&self.poisson)) {
            return Err("Poisson's ratio must be from 0 to less than 0.5".into());
        }
        if !(self.yield_strength.is_finite() && self.yield_strength > 0.0 && self.yield_strength <= 100_000.0) {
            return Err("the yield strength must be more than 0 and at most 100000 MPa".into());
        }
        Ok(())
    }
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
        if let Some(s) = &self.strength {
            s.check()?;
        }
        parse_color(&self.color).map(|_| ()).ok_or_else(|| format!("`{}` is not a colour: write it as #rrggbb", self.color))
    }

    /// How it answers to a load: its own values, or (a part saved before materials had any)
    /// those of the library's material of its name.
    pub fn strength(&self) -> Option<Strength> {
        self.strength.or_else(|| strength_of(&self.name))
    }
}

/// How the library's materials answer to a load: name, Young's modulus (GPa), Poisson's ratio,
/// yield strength (MPa). Typical values for the commonest grade and state: mild steel as
/// rolled, 304 stainless annealed, grey cast iron (its breaking stress in tension), 6061-T6,
/// soft brass, bronze and copper, Ti-6Al-4V, plastics as moulded and dry (printed parts are
/// weaker across their layers). Not listed: woods and boards, whose strength follows the
/// grain, and TPU, which stretches far beyond what a linear check can say anything about.
pub const STRENGTH: [(&str, f64, f64, f64); 17] = [
    ("Steel, mild", 200.0, 0.29, 250.0),
    ("Stainless steel", 193.0, 0.29, 215.0),
    ("Cast iron", 110.0, 0.26, 200.0),
    ("Aluminium 6061", 69.0, 0.33, 275.0),
    ("Brass", 110.0, 0.34, 125.0),
    ("Bronze", 110.0, 0.34, 130.0),
    ("Copper", 117.0, 0.34, 70.0),
    ("Titanium", 114.0, 0.34, 880.0),
    ("ABS", 2.2, 0.35, 40.0),
    ("PLA", 3.5, 0.36, 50.0),
    ("PETG", 2.1, 0.38, 50.0),
    ("Nylon (PA6)", 2.7, 0.39, 70.0),
    ("Polycarbonate", 2.3, 0.37, 62.0),
    ("Acetal (POM)", 2.9, 0.35, 65.0),
    ("Acrylic (PMMA)", 3.1, 0.37, 70.0),
    ("HDPE", 1.0, 0.42, 26.0),
    ("Resin, standard", 2.8, 0.35, 65.0),
];

/// How the library's material of this name answers to a load, if the library says.
pub fn strength_of(name: &str) -> Option<Strength> {
    let wanted = name.trim().to_lowercase();
    STRENGTH.iter().find(|m| m.0.to_lowercase() == wanted).map(|(_, e, nu, y)| Strength { youngs_modulus: *e, poisson: *nu, yield_strength: *y })
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
        strength: strength_of(name),
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
        // How a material answers to a load: every entry is of a library material and is in
        // range; steel is stiffer than aluminium, which is stiffer than any plastic; wood,
        // whose strength follows its grain, has none.
        for (name, ..) in STRENGTH {
            let m = find(name).unwrap_or_else(|| panic!("{name} is not in the library"));
            assert!(m.strength.is_some_and(|s| s.check().is_ok()), "{name}");
        }
        let stiff = |name: &str| find(name).and_then(|m| m.strength()).map(|s| s.youngs_modulus);
        assert!(stiff("Steel, mild") > stiff("Aluminium 6061") && stiff("Aluminium 6061") > stiff("PLA") && stiff("PLA") > stiff("HDPE"));
        assert_eq!((stiff("Oak"), stiff("TPU")), (None, None));
        // A part saved before materials had strengths says only its material's name: the
        // library's values are taken for it.
        let old = Material { name: "Brass".into(), density: 8.5, color: "#c9a64a".into(), strength: None };
        assert_eq!(old.strength().map(|s| s.yield_strength), Some(125.0));
        assert_eq!(serde_json::to_value(&old).unwrap(), serde_json::json!({ "name": "Brass", "density": 8.5, "color": "#c9a64a" }));
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
        let ok = Material { name: "Mine".into(), density: 2.0, color: "#102030".into(), strength: None };
        let strong =
            |e: f64, nu: f64, y: f64| Material { strength: Some(Strength { youngs_modulus: e, poisson: nu, yield_strength: y }), ..ok.clone() };
        strong(70.0, 0.33, 200.0).check().unwrap();
        assert!(strong(0.0, 0.33, 200.0).check().unwrap_err().contains("Young's modulus"));
        assert!(strong(70.0, 0.5, 200.0).check().unwrap_err().contains("Poisson"));
        assert!(strong(70.0, -0.1, 200.0).check().is_err());
        assert!(strong(70.0, 0.3, f64::NAN).check().unwrap_err().contains("yield strength"));
        ok.check().unwrap();
        assert!(Material { name: " ".into(), ..ok.clone() }.check().is_err());
        assert!(Material { density: 0.0, ..ok.clone() }.check().unwrap_err().contains("more than 0"));
        assert!(Material { density: 40.0, ..ok.clone() }.check().is_err());
        assert!(Material { density: f64::NAN, ..ok.clone() }.check().is_err());
        assert!(Material { color: "blue".into(), ..ok }.check().unwrap_err().contains("#rrggbb"));
    }
}
