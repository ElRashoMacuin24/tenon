//! Application Options, kept between sessions (DEC-101).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::Workbench;

/// Application Options as they are kept. Each that is missing is left as Tenon starts.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub show_cube: Option<bool>,
    pub show_navbar: Option<bool>,
    pub show_browser: Option<bool>,
    pub autoproject: Option<bool>,
    /// Where the stress programs are, when the user chose (otherwise they are looked for).
    pub gmsh: Option<PathBuf>,
    pub ccx: Option<PathBuf>,
}

impl Settings {
    pub fn to_text(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// Settings as kept; what cannot be read is left as Tenon starts.
    pub fn from_text(text: &str) -> Settings {
        serde_json::from_str(text).unwrap_or_default()
    }
}

/// A path typed in a box: nothing typed is no choice.
fn chosen(text: &str) -> Option<PathBuf> {
    let t = text.trim().trim_matches('"');
    (!t.is_empty()).then(|| PathBuf::from(t))
}

impl Workbench {
    /// Application Options as they are now.
    pub fn settings(&self) -> Settings {
        let c = &self.chrome;
        Settings {
            show_cube: Some(c.show_cube),
            show_navbar: Some(c.show_navbar),
            show_browser: Some(c.show_browser),
            autoproject: Some(c.autoproject),
            gmsh: chosen(&c.programs[0]),
            ccx: chosen(&c.programs[1]),
        }
    }

    /// Takes Application Options as they were kept.
    pub fn apply_settings(&mut self, s: &Settings) {
        let c = &mut self.chrome;
        for (kept, now) in [
            (s.show_cube, &mut c.show_cube),
            (s.show_navbar, &mut c.show_navbar),
            (s.show_browser, &mut c.show_browser),
            (s.autoproject, &mut c.autoproject),
        ] {
            if let Some(v) = kept {
                *now = v;
            }
        }
        let path = |p: &Option<PathBuf>| p.as_ref().map(|p| p.display().to_string()).unwrap_or_default();
        c.programs = [path(&s.gmsh), path(&s.ccx)];
        self.choose_programs();
    }

    /// Tells the stress pipeline where the user chose the programs to be.
    pub(crate) fn choose_programs(&self) {
        tenon_fea::tools::choose(chosen(&self.chrome.programs[0]), chosen(&self.chrome.programs[1]));
    }

    /// The Stress Analysis part of Application Options: where the two programs are.
    pub(crate) fn program_options(&mut self, ui: &mut egui::Ui) {
        ui.strong("Stress Analysis");
        ui.label(
            egui::RichText::new(
                "Gmsh and CalculiX work studies out (they are not part of Tenon). Leave a box empty to look for the program on the search path.",
            )
            .small(),
        );
        let (gmsh, ccx) = tenon_fea::tools::located();
        let mut changed = false;
        egui::Grid::new("tn_options_programs").num_columns(3).spacing([8.0, 4.0]).show(ui, |ui| {
            for (i, label, key, found) in [(0, "Gmsh", "tn_options_gmsh", gmsh), (1, "CalculiX", "tn_options_ccx", ccx)] {
                ui.label(label);
                let r = ui.add(
                    egui::TextEdit::singleline(&mut self.chrome.programs[i])
                        .id(egui::Id::new(key))
                        .desired_width(230.0)
                        .hint_text("on the search path"),
                );
                changed |= r.changed();
                if ui.button("Browse...").on_hover_text("Choose the program").clicked()
                    && let Some(pick) = self.services.pick_program.as_ref().and_then(|pick| pick())
                {
                    self.chrome.programs[i] = pick.display().to_string();
                    changed = true;
                }
                ui.end_row();
                ui.label("");
                let said = match (&found, chosen(&self.chrome.programs[i])) {
                    (Some(p), Some(c)) if *p != c && !c.is_file() => format!("Not there: using {}", p.display()),
                    (Some(p), _) => format!("Using {}", p.display()),
                    (None, Some(_)) => "Not there, and not on the search path".to_owned(),
                    (None, None) => "Not found on the search path".to_owned(),
                };
                ui.label(egui::RichText::new(said).small());
                ui.end_row();
            }
        });
        if changed {
            self.choose_programs();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{Driver, pressable, type_into};

    #[test]
    fn application_options_are_kept_and_say_where_the_stress_programs_are() {
        let mut wb = Workbench::headless(Box::new(tenon_kernel_occt::OcctKernel::new()));
        let mut d = Driver::new(egui::vec2(1400.0, 860.0));
        d.frame(&mut wb, vec![]);
        // File > Options...: the orientation cube off, and Gmsh where it is not.
        let file = pressable(&d, "tn_file_tab");
        d.click(&mut wb, file);
        d.frame(&mut wb, vec![]);
        let options = pressable(&d, "tools.options");
        d.click(&mut wb, options);
        d.frame(&mut wb, vec![]);
        d.frame(&mut wb, vec![]);
        let cube = d.text_pos("Orientation cube").expect("the options window");
        d.click(&mut wb, cube);
        let nowhere = std::env::temp_dir().join("no-such-folder-for-tenon").join("gmsh.exe");
        type_into(&mut d, &mut wb, "tn_options_gmsh", &nowhere.display().to_string());
        d.frame(&mut wb, vec![]);
        d.frame(&mut wb, vec![]);
        let kept = wb.settings();
        assert_eq!((kept.show_cube, kept.gmsh.as_ref(), kept.ccx.as_ref()), (Some(false), Some(&nowhere), None));
        assert!(d.texts().iter().any(|t| t.starts_with("Not there")), "it says the program is not there: {:?}", d.texts());

        // Kept as text and taken by the next Tenon to start.
        let mut next = Workbench::headless(Box::new(tenon_kernel_occt::OcctKernel::new()));
        next.apply_settings(&Settings::from_text(&kept.to_text()));
        assert_eq!(next.settings(), kept);
        assert!(!next.chrome.show_cube);
        // What cannot be read leaves Tenon as it starts.
        assert_eq!(Settings::from_text("not settings"), Settings::default());
        next.apply_settings(&Settings::default());
        assert!(!next.chrome.show_cube, "what is not said is left as it is");
        tenon_fea::tools::choose(None, None);
    }
}
