//! The radial menu: right-click in the viewport. Eight commands sit around the pointer (N, NE,
//! E, ... clockwise) and more are listed below. A right-button flick towards a slot picks it
//! without waiting for the menu. The commands depend on what you are doing: modelling, a
//! feature panel, or sketching.

use egui::{Align2, Pos2, Rect, Sense, Stroke, Ui, pos2, vec2};

use crate::commands;
use crate::theme::{self, Tokens};
use crate::workbench::{Mode, Workbench};

/// Distance of the slots from the centre.
const RADIUS: f32 = 82.0;
/// A flick shorter than this opens the menu instead of picking.
const FLICK: f32 = 36.0;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RadialEntry {
    pub label: String,
    pub id: &'static str,
}

#[derive(Clone, Debug)]
pub(crate) struct Radial {
    pub center: Pos2,
    /// N, NE, E, SE, S, SW, W, NW.
    pub slots: [Option<RadialEntry>; 8],
    pub more: Vec<RadialEntry>,
}

fn entry(id: &'static str) -> Option<RadialEntry> {
    let label = match id {
        "ui.ok" => "OK".to_owned(),
        "ui.cancel" => "Cancel".to_owned(),
        "view.previous" => "Previous View".to_owned(),
        _ => commands::find(id).filter(|c| c.available()).map(|c| c.label.to_owned())?,
    };
    Some(RadialEntry { label, id })
}

/// Unit direction of slot `i` (0 = north, clockwise).
fn slot_dir(i: usize) -> egui::Vec2 {
    let a = (i as f32) * std::f32::consts::FRAC_PI_4 - std::f32::consts::FRAC_PI_2;
    vec2(a.cos(), a.sin())
}

/// The slot a flick from `from` to `to` points at.
pub(crate) fn flick_slot(from: Pos2, to: Pos2) -> Option<usize> {
    let d = to - from;
    if d.length() < FLICK {
        return None;
    }
    let a = d.y.atan2(d.x) + std::f32::consts::FRAC_PI_2; // 0 = north
    Some(((a / std::f32::consts::FRAC_PI_4).round() as i32).rem_euclid(8) as usize)
}

impl Workbench {
    /// The menu for the current situation.
    pub(crate) fn radial_entries(&self, center: Pos2) -> Radial {
        let repeat = self.last_command.and_then(|id| entry(id).map(|e| RadialEntry { label: format!("Repeat {}", e.label), id: "ui.repeat" }));
        let (slots, more): ([Option<RadialEntry>; 8], Vec<&'static str>) = if self.panel.is_some() {
            (
                [entry("ui.ok"), None, entry("view.fit"), None, entry("ui.cancel"), None, entry("view.previous"), None],
                vec!["view.home", "view.look_at"],
            )
        } else if matches!(self.mode, Mode::Sketch(_)) {
            (
                [
                    entry("ui.ok"),
                    entry("sketch.dimension"),
                    entry("sketch.line"),
                    entry("sketch.rectangle"),
                    entry("sketch.finish"),
                    entry("sketch.circle"),
                    entry("sketch.trim"),
                    entry("view.look_at"),
                ],
                vec!["edit.undo", "edit.redo", "view.fit", "view.previous"],
            )
        } else {
            (
                [
                    repeat,
                    entry("view.look_at"),
                    entry("model.extrude"),
                    entry("model.revolve"),
                    entry("view.previous"),
                    entry("view.home"),
                    entry("sketch.new"),
                    entry("view.fit"),
                ],
                vec!["inspect.mass", "edit.undo", "edit.redo"],
            )
        };
        Radial { center, slots, more: more.into_iter().filter_map(entry).collect() }
    }

    pub(crate) fn open_radial(&mut self, at: Pos2) {
        self.chrome.radial = Some(self.radial_entries(at));
    }

    /// Picks the slot a right-button flick points at; false (menu stays open) for a short one.
    pub(crate) fn radial_flick(&mut self, from: Pos2, to: Pos2) -> bool {
        let Some(slot) = flick_slot(from, to) else { return false };
        let id = self.chrome.radial.as_ref().and_then(|r| r.slots[slot].as_ref().map(|e| e.id));
        self.chrome.radial = None;
        if let Some(id) = id {
            self.command(id);
        }
        true
    }

    pub(crate) fn radial_ui(&mut self, ui: &Ui, t: &Tokens) {
        let Some(menu) = self.chrome.radial.clone() else { return };
        let mut chosen: Option<&'static str> = None;
        let mut inside_any = false;
        let pointer = ui.input(|i| i.pointer.hover_pos());
        egui::Area::new(egui::Id::new("tn_radial")).order(egui::Order::Foreground).fixed_pos(pos2(0.0, 0.0)).show(ui.ctx(), |ui| {
            let p = ui.painter();
            p.circle_filled(menu.center, 6.0, t.panel);
            p.circle_stroke(menu.center, 6.0, Stroke::new(1.0, t.border));
            for (i, slot) in menu.slots.iter().enumerate() {
                let Some(e) = slot else { continue };
                let dir = slot_dir(i);
                let galley = p.layout_no_wrap(e.label.clone(), theme::body(), t.text);
                let size = galley.size() + vec2(18.0, 10.0);
                // Side slots grow away from the centre so long labels never cover it.
                let anchor = menu.center + dir * RADIUS;
                let min = pos2(
                    if dir.x > 0.3 {
                        anchor.x - 10.0
                    } else if dir.x < -0.3 {
                        anchor.x - size.x + 10.0
                    } else {
                        anchor.x - size.x / 2.0
                    },
                    anchor.y - size.y / 2.0,
                );
                let r = Rect::from_min_size(min, size);
                let resp = ui.interact(r, ui.id().with(("radial", i)), Sense::click());
                inside_any |= pointer.is_some_and(|q| r.contains(q));
                let hot = resp.hovered();
                p.line_segment([menu.center + dir * 8.0, anchor - dir * 4.0], Stroke::new(1.0, t.border));
                p.rect_filled(r, 4.0, if hot { t.hover } else { t.panel });
                p.rect_stroke(r, 4.0, Stroke::new(1.0, if hot { t.accent } else { t.border }), egui::StrokeKind::Inside);
                p.galley(r.min + vec2(9.0, 5.0), galley, t.text);
                if resp.clicked() {
                    chosen = Some(e.id);
                }
            }
            if !menu.more.is_empty() {
                let top = menu.center + vec2(-70.0, RADIUS + 26.0);
                let w = 140.0;
                for (j, e) in menu.more.iter().enumerate() {
                    let r = Rect::from_min_size(top + vec2(0.0, j as f32 * 24.0), vec2(w, 24.0));
                    let resp = ui.interact(r, ui.id().with(("radial-more", j)), Sense::click());
                    inside_any |= pointer.is_some_and(|q| r.contains(q));
                    p.rect_filled(r, 0.0, if resp.hovered() { t.hover } else { t.panel });
                    p.text(r.left_center() + vec2(10.0, 0.0), Align2::LEFT_CENTER, &e.label, theme::body(), t.text);
                    if resp.clicked() {
                        chosen = Some(e.id);
                    }
                }
                let all = Rect::from_min_size(top, vec2(w, menu.more.len() as f32 * 24.0));
                p.rect_stroke(all, 0.0, Stroke::new(1.0, t.border), egui::StrokeKind::Outside);
            }
        });
        let (esc, pressed) = ui.input(|i| (i.key_pressed(egui::Key::Escape), i.pointer.any_pressed()));
        if let Some(id) = chosen {
            self.chrome.radial = None;
            self.command(id);
        } else if esc || (pressed && !inside_any && !ui.input(|i| i.pointer.secondary_down())) {
            self.chrome.radial = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flicks_pick_the_slot_they_point_at() {
        let c = pos2(100.0, 100.0);
        assert_eq!(flick_slot(c, c + vec2(0.0, -80.0)), Some(0), "north");
        assert_eq!(flick_slot(c, c + vec2(80.0, 0.0)), Some(2), "east");
        assert_eq!(flick_slot(c, c + vec2(0.0, 80.0)), Some(4), "south");
        assert_eq!(flick_slot(c, c + vec2(-80.0, 0.0)), Some(6), "west");
        assert_eq!(flick_slot(c, c + vec2(-60.0, -60.0)), Some(7), "north-west");
        assert_eq!(flick_slot(c, c + vec2(10.0, 5.0)), None, "too short: open the menu");
    }
}
