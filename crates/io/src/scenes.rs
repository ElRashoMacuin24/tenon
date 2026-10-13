//! Scenes kept on disk: what a part looked like when it was last rebuilt, so that it can be
//! shown at once when it is opened again (DEC-091).
//!
//! This is a cache and nothing else. It is not part of any file format, lives in the user's
//! cache folder, can be deleted at any time, and is never trusted: whatever is read from it is
//! shown only until the part has been rebuilt from its features, which confirms it or puts
//! what is right in its place (`tenon_model::worker`). A file is found by what the document
//! says, how finely it is meshed and which build of the program wrote it, so a document that
//! changed, or a newer Tenon, simply finds nothing.
//!
//! A file is: `TNSC`, a format number, the scene without its meshes as JSON (numbers written
//! so that they read back to the last bit), then each body's meshes as plain little-endian
//! numbers, then a sum over all of it. Anything that does not read back whole is no scene.

use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tenon_kernel::{EdgePolyline, FaceRange, Mesh, MeshTol};
use tenon_model::worker::SceneStore;
use tenon_model::{Document, Scene};

const MAGIC: &[u8; 4] = b"TNSC";
/// Raised whenever what is written changes: older files are then not found.
const FORMAT: u32 = 1;
/// The most the folder holds before the files unused longest are deleted.
pub const DEFAULT_LIMIT: u64 = 256 << 20;

/// A folder of kept scenes.
#[derive(Clone, Debug)]
pub struct SceneFiles {
    dir: PathBuf,
    stamp: u64,
    limit: u64,
}

/// Which build of the program this is: its version, and the size and date of the program
/// file. A build that might come to something else finds nothing of another's.
fn program_stamp() -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    env!("CARGO_PKG_VERSION").hash(&mut h);
    if let Some(meta) = std::env::current_exe().ok().and_then(|p| std::fs::metadata(p).ok()) {
        meta.len().hash(&mut h);
        meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_nanos()).hash(&mut h);
    }
    h.finish()
}

impl SceneFiles {
    /// Scenes kept in `dir` (made when the first is written).
    pub fn at(dir: impl Into<PathBuf>) -> SceneFiles {
        SceneFiles { dir: dir.into(), stamp: program_stamp(), limit: DEFAULT_LIMIT }
    }

    /// The same, holding at most `bytes`.
    pub fn with_limit(mut self, bytes: u64) -> SceneFiles {
        self.limit = bytes;
        self
    }

    /// The same, as another build of the program would see it (tests).
    pub fn with_stamp(mut self, stamp: u64) -> SceneFiles {
        self.stamp = stamp;
        self
    }

    /// Where scenes are kept when nothing says otherwise: the user's cache folder.
    ///
    /// - Windows: `%LOCALAPPDATA%\Tenon\cache\scenes`
    /// - macOS: `~/Library/Caches/Tenon/scenes`
    /// - elsewhere: `$XDG_CACHE_HOME/tenon/scenes`, or `~/.cache/tenon/scenes`
    ///
    /// `TENON_SCENE_CACHE` names another folder, or with `off` says to keep none.
    pub fn default_dir() -> Option<PathBuf> {
        let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from);
        if let Some(said) = var("TENON_SCENE_CACHE") {
            return (said != Path::new("off")).then_some(said);
        }
        if cfg!(target_os = "windows") {
            var("LOCALAPPDATA").map(|d| d.join("Tenon").join("cache").join("scenes"))
        } else if cfg!(target_os = "macos") {
            var("HOME").map(|d| d.join("Library").join("Caches").join("Tenon").join("scenes"))
        } else {
            var("XDG_CACHE_HOME").or_else(|| var("HOME").map(|d| d.join(".cache"))).map(|d| d.join("tenon").join("scenes"))
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The file a scene of `doc` is kept in.
    fn file(&self, doc: &Document, tol: &MeshTol, far: bool) -> Option<PathBuf> {
        // What the document says, as its file would say it.
        let text = crate::project::to_bytes(doc, &serde_json::Map::new()).ok()?;
        let tol = serde_json::to_vec(tol).ok()?;
        let half = |salt: u64| {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            (salt, FORMAT, self.stamp, far).hash(&mut h);
            tol.hash(&mut h);
            text.hash(&mut h);
            h.finish()
        };
        Some(self.dir.join(format!("{:016x}{:016x}.scene", half(0x9e37_79b9_7f4a_7c15), half(0xc2b2_ae3d_27d4_eb4f))))
    }

    /// Deletes the files unused longest until the folder holds no more than it may. `keep` is
    /// never deleted.
    fn trim(&self, keep: &Path) {
        let Ok(entries) = std::fs::read_dir(&self.dir) else { return };
        let mut files: Vec<(std::time::SystemTime, u64, PathBuf)> = entries
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "scene"))
            .filter_map(|e| {
                let meta = e.metadata().ok()?;
                Some((meta.modified().ok()?, meta.len(), e.path()))
            })
            .collect();
        let mut held: u64 = files.iter().map(|f| f.1).sum();
        files.sort();
        for (_, bytes, path) in files {
            if held <= self.limit {
                break;
            }
            if path != keep && std::fs::remove_file(&path).is_ok() {
                held -= bytes;
            }
        }
    }
}

impl SceneStore for SceneFiles {
    fn get(&self, doc: &Document, tol: &MeshTol, far: bool) -> Option<Scene> {
        let path = self.file(doc, tol, far)?;
        let bytes = std::fs::read(&path).ok()?;
        let Some(scene) = decode(&bytes) else {
            // (Cut short, or damaged: of no use to anyone.)
            let _ = std::fs::remove_file(&path);
            return None;
        };
        // Used now: among the last to go when the folder is full.
        if let Ok(f) = std::fs::File::options().write(true).open(&path) {
            let _ = f.set_modified(std::time::SystemTime::now());
        }
        Some(scene)
    }

    fn put(&self, doc: &Document, tol: &MeshTol, far: bool, scene: &Scene) {
        let (Some(path), Some(bytes)) = (self.file(doc, tol, far), encode(scene)) else { return };
        if bytes.len() as u64 > self.limit / 4 || std::fs::create_dir_all(&self.dir).is_err() {
            return;
        }
        // Written beside and moved into place, so that a reader never finds half a file.
        let beside = path.with_extension(format!("{}.part", std::process::id()));
        if std::fs::write(&beside, &bytes).is_ok() && std::fs::rename(&beside, &path).is_ok() {
            self.trim(&path);
        } else {
            let _ = std::fs::remove_file(&beside);
        }
    }
}

fn sum(bytes: &[u8]) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut h);
    h.finish()
}

fn put_count(out: &mut Vec<u8>, n: usize) -> Option<()> {
    out.extend(u32::try_from(n).ok()?.to_le_bytes());
    Some(())
}

fn put_points(out: &mut Vec<u8>, points: &[[f32; 3]]) -> Option<()> {
    put_count(out, points.len())?;
    out.reserve(points.len() * 12);
    for p in points {
        for c in p {
            out.extend(c.to_le_bytes());
        }
    }
    Some(())
}

fn put_mesh(out: &mut Vec<u8>, m: &Mesh) -> Option<()> {
    put_points(out, &m.positions)?;
    put_points(out, &m.normals)?;
    put_count(out, m.indices.len())?;
    out.reserve(m.indices.len() * 4);
    m.indices.iter().for_each(|i| out.extend(i.to_le_bytes()));
    put_count(out, m.faces.len())?;
    for f in &m.faces {
        [f.face, f.first, f.count].iter().for_each(|v| out.extend(v.to_le_bytes()));
    }
    put_count(out, m.edges.len())?;
    for e in &m.edges {
        out.extend(e.edge.to_le_bytes());
        put_points(out, &e.points)?;
    }
    Some(())
}

/// A scene as the bytes of its file. None for a scene that cannot be written (a number that
/// is not one, more of something than a count holds).
pub fn encode(scene: &Scene) -> Option<Vec<u8>> {
    let json = serde_json::to_vec(scene).ok()?;
    let mut out = Vec::with_capacity(json.len() + 64);
    out.extend(MAGIC);
    out.extend(FORMAT.to_le_bytes());
    out.extend((json.len() as u64).to_le_bytes());
    out.extend(&json);
    put_count(&mut out, scene.bodies.len())?;
    for b in &scene.bodies {
        put_mesh(&mut out, &b.mesh)?;
        match &b.far {
            Some(far) => {
                out.push(1);
                put_mesh(&mut out, far)?;
            }
            None => out.push(0),
        }
    }
    let check = sum(&out);
    out.extend(check.to_le_bytes());
    Some(out)
}

/// What is left of a file being read.
struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let (head, rest) = self.0.split_at_checked(n)?;
        self.0 = rest;
        Some(head)
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    fn u64(&mut self) -> Option<u64> {
        Some(u64::from_le_bytes(self.take(8)?.try_into().ok()?))
    }
    /// A count of things each at least `each` bytes long: no more than there is file left
    /// for, so that a damaged count cannot ask for memory the file does not fill.
    fn count(&mut self, each: usize) -> Option<usize> {
        let n = self.u32()? as usize;
        (n.checked_mul(each)? <= self.0.len()).then_some(n)
    }
    fn points(&mut self) -> Option<Vec<[f32; 3]>> {
        let n = self.count(12)?;
        let bytes = self.take(n * 12)?;
        let (triples, _) = bytes.as_chunks::<12>();
        let point = |&[a, b, c, d, e, f, g, h, i, j, k, l]: &[u8; 12]| {
            [f32::from_le_bytes([a, b, c, d]), f32::from_le_bytes([e, f, g, h]), f32::from_le_bytes([i, j, k, l])]
        };
        Some(triples.iter().map(point).collect())
    }
    fn mesh(&mut self) -> Option<Mesh> {
        let positions = self.points()?;
        let normals = self.points()?;
        let n = self.count(4)?;
        let (fours, _) = self.take(n * 4)?.as_chunks::<4>();
        let indices: Vec<u32> = fours.iter().map(|b| u32::from_le_bytes(*b)).collect();
        let n = self.count(12)?;
        let mut faces = Vec::with_capacity(n);
        for _ in 0..n {
            faces.push(FaceRange { face: self.u32()?, first: self.u32()?, count: self.u32()? });
        }
        let n = self.count(8)?;
        let mut edges = Vec::with_capacity(n);
        for _ in 0..n {
            edges.push(EdgePolyline { edge: self.u32()?, points: self.points()? });
        }
        Some(Mesh { positions, normals, indices, faces, edges, ..Mesh::default() })
    }
}

/// The scene a file holds. None for anything that is not a whole file of this format.
pub fn decode(bytes: &[u8]) -> Option<Scene> {
    let (body, check) = bytes.split_at_checked(bytes.len().checked_sub(8)?)?;
    if u64::from_le_bytes(check.try_into().ok()?) != sum(body) {
        return None;
    }
    let mut r = Reader(body);
    if r.take(4)? != MAGIC || r.u32()? != FORMAT {
        return None;
    }
    let json = usize::try_from(r.u64()?).ok()?;
    let mut scene: Scene = serde_json::from_slice(r.take(json)?).ok()?;
    if r.count(1)? != scene.bodies.len() {
        return None;
    }
    for b in &mut scene.bodies {
        b.mesh = Arc::new(r.mesh()?);
        b.far = match r.take(1)? {
            [0] => None,
            [1] => Some(Arc::new(r.mesh()?)),
            _ => return None,
        };
    }
    r.0.is_empty().then_some(scene)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use tenon_model::BodyView;

    fn mesh(shift: f32) -> Mesh {
        Mesh {
            positions: vec![[shift, 0.0, 0.1], [1.0, 0.0, 0.0], [0.0, 1.0, 1.0e-7], [0.3, 0.3, 0.3]],
            normals: vec![[0.0, 0.0, 1.0]; 4],
            indices: vec![0, 1, 2, 0, 2, 3],
            faces: vec![FaceRange { face: 0, first: 0, count: 3 }, FaceRange { face: 1, first: 3, count: 3 }],
            edges: vec![EdgePolyline { edge: 0, points: vec![[shift, 0.0, 0.1], [1.0, 0.0, 0.0]] }, EdgePolyline { edge: 1, points: vec![] }],
            ..Mesh::default()
        }
    }

    /// A scene with a number that text would not carry to the last bit unless care is taken.
    fn scene() -> Scene {
        let third = 1.0_f64 / 3.0;
        let body = |shift: f32, far: bool| BodyView {
            mesh: Arc::new(mesh(shift)),
            far: far.then(|| Arc::new(mesh(shift + 0.5))),
            at: None,
            faces: Vec::new(),
            volume: third,
            mass: tenon_kernel::MassProps {
                volume: third,
                area: 0.1 + 0.2,
                mass: third,
                center_of_mass: tenon_geom::Vec3::new(third, -third, 1e-300),
                inertia: [[third; 3]; 3],
            },
            bbox: tenon_geom::Aabb3::from_points([tenon_geom::Vec3::ZERO, tenon_geom::Vec3::new(1.0, 1.0, third)]),
            edges: Vec::new(),
            curves: Vec::new(),
            ends: vec![[tenon_geom::Vec3::ZERO, tenon_geom::Vec3::X]],
            mids: vec![tenon_geom::Vec3::new(0.5, 0.0, 0.0)],
        };
        Scene { bodies: vec![body(0.0, true), body(2.0, false)], far: true, regen_ms: 12.5, ..Scene::default() }
    }

    #[test]
    fn a_scene_reads_back_as_it_was_written_to_the_last_bit() {
        let s = scene();
        let bytes = encode(&s).unwrap();
        let back = decode(&bytes).unwrap();
        assert_eq!(back, s);
        assert!(back.same_as(&s));
        assert_eq!(back.bodies[0].mass.center_of_mass.x.to_bits(), (1.0_f64 / 3.0).to_bits());
        assert_eq!(back.bodies[0].mesh.positions[2][2].to_bits(), 1.0e-7_f32.to_bits());
        assert!(back.bodies[0].far.is_some() && back.bodies[1].far.is_none());
        // Anything that is not the whole file is no scene: cut short at every length, a byte
        // changed anywhere, something after the end.
        for cut in 0..bytes.len() {
            assert!(decode(&bytes[..cut]).is_none(), "cut at {cut}");
        }
        for at in (0..bytes.len()).step_by(7) {
            let mut damaged = bytes.clone();
            damaged[at] ^= 0x40;
            assert!(decode(&damaged).is_none(), "damaged at {at}");
        }
        let mut longer = bytes.clone();
        longer.push(0);
        assert!(decode(&longer).is_none());
        assert!(decode(b"").is_none() && decode(b"not a scene at all, but long enough").is_none());
        // A count that asks for more than the file holds is refused before memory is asked for.
        let mut greedy = bytes[..bytes.len() - 8].to_vec();
        let at = 4 + 4 + 8 + serde_json::to_vec(&s).unwrap().len() + 4;
        greedy[at..at + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        let check = sum(&greedy);
        greedy.extend(check.to_le_bytes());
        assert!(decode(&greedy).is_none());
        // A scene with a number that is not one is not written.
        let mut bad = scene();
        bad.bodies[0].volume = f64::NAN;
        assert!(encode(&bad).and_then(|b| decode(&b)).is_none());
    }

    #[test]
    fn scenes_are_kept_by_what_the_document_says_and_the_oldest_go_when_the_folder_is_full() {
        let dir = std::env::temp_dir().join(format!("tenon-io-scenes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = SceneFiles::at(&dir);
        let tol = MeshTol::default();
        let named = |name: &str| {
            let mut d = Document::default();
            d.name = name.to_owned();
            d
        };
        let (a, b) = (named("a"), named("b"));
        // Nothing kept: nothing found, and no folder made by looking.
        assert!(store.get(&a, &tol, false).is_none());
        assert!(!dir.exists());
        store.put(&a, &tol, false, &scene());
        assert!(store.get(&a, &tol, false).is_some_and(|s| s.same_as(&scene())));
        // Another document, another fineness, with or without far meshes, another build of
        // the program: each finds nothing of this one's.
        assert!(store.get(&b, &tol, false).is_none());
        assert!(store.get(&a, &MeshTol { linear: tol.linear * 2.0, ..tol }, false).is_none());
        assert!(store.get(&a, &tol, true).is_none());
        assert!(store.clone().with_stamp(7).get(&a, &tol, false).is_none());
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1, "and nothing was left behind by the writing");
        // A file that was damaged is no scene, and is cleared away.
        let file = std::fs::read_dir(&dir).unwrap().next().unwrap().unwrap().path();
        let mut bytes = std::fs::read(&file).unwrap();
        let middle = bytes.len() / 2;
        bytes[middle] ^= 1;
        std::fs::write(&file, &bytes).unwrap();
        assert!(store.get(&a, &tol, false).is_none());
        assert!(!file.exists());

        // The folder holds so much and no more: the scenes unused longest go first.
        let one = encode(&scene()).unwrap().len() as u64;
        let small = SceneFiles::at(&dir).with_limit(one * 4 + one / 2);
        let docs: Vec<Document> = (0..6).map(|i| named(&format!("part {i}"))).collect();
        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
        for (i, d) in docs.iter().enumerate() {
            small.put(d, &tol, false, &scene());
            // (Each an hour old and a minute newer than the last, as if written over an hour.)
            let f = std::fs::File::options().write(true).open(small.file(d, &tol, false).unwrap());
            if let Ok(f) = f {
                f.set_modified(old + std::time::Duration::from_secs(60 * i as u64)).unwrap();
            }
        }
        // (Six written into room for four: the first two went as the fifth and sixth came.)
        let kept: Vec<bool> = docs.iter().map(|d| small.file(d, &tol, false).unwrap().exists()).collect();
        assert_eq!(kept, [false, false, true, true, true, true]);
        // One that is read is used now, and outlasts the others.
        assert!(small.get(&docs[2], &tol, false).is_some());
        small.put(&named("one more"), &tol, false, &scene());
        let kept: Vec<bool> = docs.iter().map(|d| small.file(d, &tol, false).unwrap().exists()).collect();
        assert_eq!(kept, [false, false, true, false, true, true]);
        // A scene too large for the folder is not kept at all.
        SceneFiles::at(&dir).with_limit(one).put(&named("too large"), &tol, false, &scene());
        assert!(!SceneFiles::at(&dir).file(&named("too large"), &tol, false).unwrap().exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
