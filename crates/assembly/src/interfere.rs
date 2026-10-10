//! Work on placed components' solids through the kernel: interference, STEP export of the whole
//! assembly.

use tenon_geom::{Aabb3, Axis, Frame, Vec3, tol};
use tenon_kernel::{BoolOp, Kernel, KernelError, Mesh, MeshTol, ShapeHandle, Transform};

use crate::math::M3;
use crate::model::{Assembly, ComponentId};
use crate::session::{Parts, local_bbox, placed_bbox};

/// A copy of `shape` placed by `f` (part coordinates to assembly coordinates). The caller
/// releases it.
pub fn place(k: &mut dyn Kernel, shape: ShapeHandle, f: &Frame) -> Result<ShapeHandle, KernelError> {
    let (axis, angle) = M3::of_frame(f).axis_angle();
    let turned = if angle.abs() > 1e-12 {
        let axis = Axis::new(Vec3::ZERO, axis).ok_or_else(|| KernelError::InvalidInput("bad rotation axis".into()))?;
        Some(k.transform(shape, &Transform::Rotate { axis, angle })?.shape)
    } else {
        None
    };
    let moved = k.transform(turned.unwrap_or(shape), &Transform::Translate(f.origin()));
    if let Some(t) = turned {
        k.release(t);
    }
    Ok(moved?.shape)
}

/// One component's solids, as the kernel holds them in part coordinates, with where they go.
#[derive(Clone, Debug)]
pub struct Placed {
    pub component: ComponentId,
    pub shapes: Vec<ShapeHandle>,
    pub frame: Frame,
    /// The part's bounding box, in part coordinates (skips pairs that cannot touch).
    pub bbox: Option<Aabb3>,
}

/// The solids of every visible component, as the parts' sessions regenerate them (they keep the
/// shapes; nothing here is to be released).
pub fn placed_parts(
    k: &mut dyn Kernel,
    asm: &Assembly,
    parts: &mut Parts,
    frames: Option<&std::collections::BTreeMap<ComponentId, Frame>>,
) -> Result<Vec<Placed>, String> {
    let mut out = Vec::new();
    for c in asm.components.iter().filter(|c| c.visible) {
        let bbox = local_bbox(parts, c);
        let part = parts.get_mut(&c.key()).ok_or_else(|| format!("{}: the part is not loaded", c.name))?;
        if let Some(why) = &part.missing {
            return Err(format!("{} is missing: {why}", c.name));
        }
        let frame = frames.and_then(|m| m.get(&c.id)).copied().unwrap_or(c.placement);
        let shapes = part.session.regen(k).bodies.iter().map(|b| b.shape).collect();
        out.push(Placed { component: c.id, shapes, frame, bbox });
    }
    Ok(out)
}

/// Placed copies of every solid; the caller releases them.
fn copies(k: &mut dyn Kernel, items: &[Placed]) -> Result<Vec<(ComponentId, ShapeHandle, Option<Aabb3>)>, String> {
    let mut out = Vec::new();
    for it in items {
        for s in &it.shapes {
            match place(k, *s, &it.frame) {
                Ok(c) => out.push((it.component, c, it.bbox.map(|b| placed_bbox(&b, &it.frame)))),
                Err(e) => {
                    for (_, c, _) in out {
                        k.release(c);
                    }
                    return Err(e.to_string());
                }
            }
        }
    }
    Ok(out)
}

/// Two components that overlap.
#[derive(Clone, Debug, PartialEq)]
pub struct Clash {
    pub a: ComponentId,
    pub b: ComponentId,
    /// Volume of the overlap (mm^3).
    pub volume: f64,
    /// The overlap, to show (assembly coordinates).
    pub mesh: Mesh,
}

/// Every pair of components (one of them among `only`, when given) whose solids overlap.
pub fn clashes(k: &mut dyn Kernel, items: &[Placed], only: Option<&[ComponentId]>) -> Result<Vec<Clash>, String> {
    let solids = copies(k, items)?;
    let mut found: Vec<Clash> = Vec::new();
    let mut err = None;
    'pairs: for i in 0..solids.len() {
        for j in i + 1..solids.len() {
            let ((ca, sa, ba), (cb, sb, bb)) = (solids[i], solids[j]);
            if ca == cb || only.is_some_and(|o| !o.contains(&ca) && !o.contains(&cb)) {
                continue;
            }
            if let (Some(a), Some(b)) = (ba, bb) {
                let apart =
                    a.max.x < b.min.x || b.max.x < a.min.x || a.max.y < b.min.y || b.max.y < a.min.y || a.max.z < b.min.z || b.max.z < a.min.z;
                if apart {
                    continue;
                }
            }
            let common = match k.boolean(BoolOp::Intersect, sa, &[sb]) {
                Ok(op) => op.shape,
                Err(e) => {
                    err = Some(e.to_string());
                    break 'pairs;
                }
            };
            let volume = k.mass_properties(common, 1.0).map(|m| m.volume).unwrap_or(0.0);
            if volume > tol::CLASH_VOLUME {
                let mesh = k.tessellate(common, &MeshTol::default()).unwrap_or_default();
                match found.iter_mut().find(|c| c.a == ca && c.b == cb) {
                    // Parts with several bodies: one clash per pair of components.
                    Some(c) => {
                        c.volume += volume;
                        let base = u32::try_from(c.mesh.positions.len()).unwrap_or(u32::MAX);
                        c.mesh.positions.extend(mesh.positions);
                        c.mesh.normals.extend(mesh.normals);
                        c.mesh.indices.extend(mesh.indices.iter().map(|i| i.saturating_add(base)));
                    }
                    None => found.push(Clash { a: ca, b: cb, volume, mesh }),
                }
            }
            k.release(common);
        }
    }
    for (_, s, _) in solids {
        k.release(s);
    }
    match err {
        Some(e) => Err(e),
        None => Ok(found),
    }
}

/// Every pair of visible components (among `only`, when given) whose solids overlap.
pub fn interference(k: &mut dyn Kernel, asm: &Assembly, parts: &mut Parts, only: Option<&[ComponentId]>) -> Result<Vec<Clash>, String> {
    let items = placed_parts(k, asm, parts, None)?;
    clashes(k, &items, only)
}

/// STEP data of solids where they are placed.
pub fn step_of(k: &mut dyn Kernel, items: &[Placed]) -> Result<Vec<u8>, String> {
    let solids = copies(k, items)?;
    if solids.is_empty() {
        return Err("there is no solid to export".into());
    }
    let shapes: Vec<ShapeHandle> = solids.iter().map(|(_, s, _)| *s).collect();
    let data = k.export_step(&shapes).map_err(|e| e.to_string());
    for s in shapes {
        k.release(s);
    }
    data
}

/// STEP data of every visible component's solids where they are placed (at `frames`, e.g. the
/// exploded view, or their placements).
pub fn export_step(
    k: &mut dyn Kernel,
    asm: &Assembly,
    parts: &mut Parts,
    frames: Option<&std::collections::BTreeMap<ComponentId, Frame>>,
) -> Result<Vec<u8>, String> {
    let items = placed_parts(k, asm, parts, frames)?;
    step_of(k, &items)
}
