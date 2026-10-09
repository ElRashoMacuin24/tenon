//! Work on placed components' solids through the kernel: interference, STEP export of the whole
//! assembly.

use tenon_geom::{Axis, Frame, Vec3, tol};
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

/// The solids of every visible component, placed (at `frames`, or their placements). The caller
/// releases them.
pub fn placed_solids(
    k: &mut dyn Kernel,
    asm: &Assembly,
    parts: &mut Parts,
    frames: Option<&std::collections::BTreeMap<ComponentId, Frame>>,
) -> Result<Vec<(ComponentId, ShapeHandle)>, String> {
    let mut out = Vec::new();
    let result = (|| {
        for c in asm.components.iter().filter(|c| c.visible) {
            let part = parts.get_mut(&c.part).ok_or_else(|| format!("{}: the part is not loaded", c.name))?;
            if let Some(why) = &part.missing {
                return Err(format!("{} is missing: {why}", c.name));
            }
            let f = frames.and_then(|m| m.get(&c.id)).copied().unwrap_or(c.placement);
            let bodies: Vec<ShapeHandle> = part.session.regen(k).bodies.iter().map(|b| b.shape).collect();
            for b in bodies {
                out.push((c.id, place(k, b, &f).map_err(|e| e.to_string())?));
            }
        }
        Ok(())
    })();
    if let Err(e) = result {
        for (_, s) in out {
            k.release(s);
        }
        return Err(e);
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

/// Every pair of visible components (among `only`, when given) whose solids overlap.
pub fn interference(k: &mut dyn Kernel, asm: &Assembly, parts: &mut Parts, only: Option<&[ComponentId]>) -> Result<Vec<Clash>, String> {
    let solids = placed_solids(k, asm, parts, None)?;
    let boxes: Vec<Option<tenon_geom::Aabb3>> =
        solids.iter().map(|(id, _)| asm.component(*id).and_then(|c| local_bbox(parts, c).map(|b| placed_bbox(&b, &c.placement)))).collect();
    let mut clashes: Vec<Clash> = Vec::new();
    let mut err = None;
    'pairs: for i in 0..solids.len() {
        for j in i + 1..solids.len() {
            let ((ca, sa), (cb, sb)) = (solids[i], solids[j]);
            if ca == cb || only.is_some_and(|o| !o.contains(&ca) && !o.contains(&cb)) {
                continue;
            }
            if let (Some(Some(a)), Some(Some(b))) = (boxes.get(i), boxes.get(j)) {
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
                match clashes.iter_mut().find(|c| c.a == ca && c.b == cb) {
                    // Parts with several bodies: one clash per pair of components.
                    Some(c) => {
                        c.volume += volume;
                        let base = u32::try_from(c.mesh.positions.len()).unwrap_or(u32::MAX);
                        c.mesh.positions.extend(mesh.positions);
                        c.mesh.normals.extend(mesh.normals);
                        c.mesh.indices.extend(mesh.indices.iter().map(|i| i.saturating_add(base)));
                    }
                    None => clashes.push(Clash { a: ca, b: cb, volume, mesh }),
                }
            }
            k.release(common);
        }
    }
    for (_, s) in solids {
        k.release(s);
    }
    match err {
        Some(e) => Err(e),
        None => Ok(clashes),
    }
}

/// STEP data of every visible component's solids where they are placed (at `frames`, e.g. the
/// exploded view, or their placements).
pub fn export_step(
    k: &mut dyn Kernel,
    asm: &Assembly,
    parts: &mut Parts,
    frames: Option<&std::collections::BTreeMap<ComponentId, Frame>>,
) -> Result<Vec<u8>, String> {
    let solids = placed_solids(k, asm, parts, frames)?;
    if solids.is_empty() {
        return Err("there is no solid to export".into());
    }
    let shapes: Vec<ShapeHandle> = solids.iter().map(|(_, s)| *s).collect();
    let data = k.export_step(&shapes).map_err(|e| e.to_string());
    for s in shapes {
        k.release(s);
    }
    data
}
