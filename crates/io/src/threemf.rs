//! 3MF output: the 3D Manufacturing Format's core, as slicers read it. A 3MF file is a zip (an
//! Open Packaging Conventions package) of three parts: the content types, the package
//! relationships and the model, which is XML. Units are millimetres, as everywhere in Tenon.
//!
//! Each object is a welded mesh (every corner once, triangles counter-clockwise from outside)
//! with a base material that carries its name and colour. A build item places an object, with a
//! transform when it is an assembly's component. The bytes written depend only on the model:
//! no clock, no machine.

use std::fmt::Write as _;
use std::io::Write as _;

use tenon_geom::Frame;

use crate::meshcheck::Welded;

/// The model part's name in the package.
pub const MODEL_PART: &str = "3D/3dmodel.model";
const CONTENT_TYPES: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="model" ContentType="application/vnd.ms-package.3dmanufacturing-3dmodel+xml"/>
</Types>
"#;
const RELATIONSHIPS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Target="/3D/3dmodel.model" Id="rel0" Type="http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel"/>
</Relationships>
"#;
/// Most triangles or corners one object may have (the format counts them in 31 bits).
const MAX_COUNT: usize = i32::MAX as usize;

/// A solid to print.
#[derive(Clone, Debug, PartialEq)]
pub struct Object {
    pub name: String,
    pub mesh: Welded,
    /// What it is made of, as the slicer's material list names it.
    pub material: String,
    /// Its colour, sRGB.
    pub color: [u8; 3],
}

/// An object placed in the build: where it is, for a component of an assembly.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    /// Index into [`Model::objects`].
    pub object: usize,
    pub frame: Option<Frame>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Model {
    pub title: String,
    pub objects: Vec<Object>,
    pub items: Vec<Item>,
}

/// Text as XML attribute or element content.
fn xml(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c if c.is_control() => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

/// A coordinate as short as it reads back the same (the mesh's positions are single precision).
fn coordinate(x: f64) -> String {
    let v = x as f32;
    if v == 0.0 { "0".into() } else { format!("{v}") }
}

/// The model part: XML.
pub fn model_xml(model: &Model) -> Result<String, String> {
    if model.objects.is_empty() {
        return Err("there is no solid to export".into());
    }
    let mut out = String::new();
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    out.push_str("<model unit=\"millimeter\" xml:lang=\"en-US\" xmlns=\"http://schemas.microsoft.com/3dmanufacturing/core/2015/02\">\n");
    let _ = writeln!(out, "  <metadata name=\"Title\">{}</metadata>", xml(&model.title));
    out.push_str("  <metadata name=\"Application\">Tenon</metadata>\n  <resources>\n");
    // One material group, a base material per object; objects follow from id 2.
    out.push_str("    <basematerials id=\"1\">\n");
    for o in &model.objects {
        let [r, g, b] = o.color;
        let _ = writeln!(out, "      <base name=\"{}\" displaycolor=\"#{r:02X}{g:02X}{b:02X}FF\"/>", xml(&o.material));
    }
    out.push_str("    </basematerials>\n");
    for (i, o) in model.objects.iter().enumerate() {
        if o.mesh.triangles.is_empty() {
            return Err(format!("{} has no surface to export", o.name));
        }
        if o.mesh.positions.len() > MAX_COUNT || o.mesh.triangles.len() > MAX_COUNT {
            return Err(format!("{} has too many triangles for a 3MF file", o.name));
        }
        if o.mesh.positions.iter().any(|p| !(p.x.is_finite() && p.y.is_finite() && p.z.is_finite())) {
            return Err(format!("{} has a corner that is not a number", o.name));
        }
        let _ = writeln!(out, "    <object id=\"{}\" type=\"model\" name=\"{}\" pid=\"1\" pindex=\"{i}\">", i + 2, xml(&o.name));
        out.push_str("      <mesh>\n        <vertices>\n");
        for p in &o.mesh.positions {
            let _ = writeln!(out, "          <vertex x=\"{}\" y=\"{}\" z=\"{}\"/>", coordinate(p.x), coordinate(p.y), coordinate(p.z));
        }
        out.push_str("        </vertices>\n        <triangles>\n");
        for t in &o.mesh.triangles {
            let _ = writeln!(out, "          <triangle v1=\"{}\" v2=\"{}\" v3=\"{}\"/>", t[0], t[1], t[2]);
        }
        out.push_str("        </triangles>\n      </mesh>\n    </object>\n");
    }
    out.push_str("  </resources>\n  <build>\n");
    for item in &model.items {
        if item.object >= model.objects.len() {
            return Err("a build item names an object that is not there".into());
        }
        match &item.frame {
            // Rows are where the object's X, Y and Z go, then its origin.
            Some(f) => {
                let rows = [f.x(), f.y(), f.z(), f.origin()];
                let m: Vec<String> =
                    rows.iter().flat_map(|v| [v.x, v.y, v.z]).map(|x| if x == 0.0 { "0".to_owned() } else { format!("{x}") }).collect();
                let _ = writeln!(out, "    <item objectid=\"{}\" transform=\"{}\"/>", item.object + 2, m.join(" "));
            }
            None => {
                let _ = writeln!(out, "    <item objectid=\"{}\"/>", item.object + 2);
            }
        }
    }
    out.push_str("  </build>\n</model>\n");
    Ok(out)
}

/// The 3MF file's bytes.
pub fn write(model: &Model) -> Result<Vec<u8>, String> {
    let xml = model_xml(model)?;
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut zw = zip::ZipWriter::new(&mut buf);
        // (A fixed date: the same model is the same file.)
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .last_modified_time(zip::DateTime::default());
        let err = |e: zip::result::ZipError| e.to_string();
        for (name, text) in [("[Content_Types].xml", CONTENT_TYPES), ("_rels/.rels", RELATIONSHIPS), (MODEL_PART, xml.as_str())] {
            zw.start_file(name, opts).map_err(err)?;
            zw.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
        }
        zw.finish().map_err(err)?;
    }
    Ok(buf.into_inner())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use std::io::Read as _;

    use tenon_geom::Vec3;

    use super::*;

    fn tetrahedron() -> Welded {
        Welded {
            positions: vec![Vec3::ZERO, Vec3::new(10.0, 0.0, 0.0), Vec3::new(0.0, 10.0, 0.0), Vec3::new(0.0, 0.0, 10.0)],
            triangles: vec![[0, 2, 1], [0, 1, 3], [1, 2, 3], [0, 3, 2]],
            faces: vec![0, 1, 2, 3],
        }
    }

    fn part() -> Model {
        Model {
            title: "A <test> & more".into(),
            objects: vec![Object { name: "Wedge \"1\"".into(), mesh: tetrahedron(), material: "PLA".into(), color: [0x4f, 0x9f, 0xd8] }],
            items: vec![Item { object: 0, frame: None }],
        }
    }

    #[test]
    fn the_package_has_its_three_parts_and_the_model_says_millimetres() {
        let bytes = write(&part()).unwrap();
        let mut za = zip::ZipArchive::new(std::io::Cursor::new(&bytes)).unwrap();
        let names: Vec<String> = (0..za.len()).map(|i| za.by_index(i).unwrap().name().to_owned()).collect();
        assert_eq!(names, ["[Content_Types].xml", "_rels/.rels", "3D/3dmodel.model"]);
        let mut text = String::new();
        za.by_name(MODEL_PART).unwrap().read_to_string(&mut text).unwrap();
        assert!(text.contains("<model unit=\"millimeter\""), "{text}");
        assert!(text.contains("xmlns=\"http://schemas.microsoft.com/3dmanufacturing/core/2015/02\""));
        // Names are escaped; the colour is written with full opacity.
        assert!(text.contains("<metadata name=\"Title\">A &lt;test&gt; &amp; more</metadata>"), "{text}");
        assert!(text.contains("<object id=\"2\" type=\"model\" name=\"Wedge &quot;1&quot;\" pid=\"1\" pindex=\"0\">"), "{text}");
        assert!(text.contains("<base name=\"PLA\" displaycolor=\"#4F9FD8FF\"/>"), "{text}");
        assert_eq!((text.matches("<vertex ").count(), text.matches("<triangle ").count()), (4, 4));
        assert!(text.contains("<vertex x=\"10\" y=\"0\" z=\"0\"/>") && text.contains("<triangle v1=\"0\" v2=\"2\" v3=\"1\"/>"), "{text}");
        assert!(text.contains("<item objectid=\"2\"/>"));
        // The relationships point at the model.
        let mut rels = String::new();
        za.by_name("_rels/.rels").unwrap().read_to_string(&mut rels).unwrap();
        assert!(rels.contains("Target=\"/3D/3dmodel.model\""));
        // The same model is the same bytes.
        assert_eq!(write(&part()).unwrap(), bytes);
    }

    #[test]
    fn components_are_items_of_one_object_each_where_it_is() {
        let mut m = part();
        let turned = Frame::new(Vec3::new(5.0, 6.0, 7.0), Vec3::new(0.0, 0.0, 1.0), Vec3::new(0.0, 1.0, 0.0)).unwrap();
        m.items = vec![Item { object: 0, frame: Some(Frame::WORLD) }, Item { object: 0, frame: Some(turned) }];
        let text = model_xml(&m).unwrap();
        assert_eq!(text.matches("<object ").count(), 1);
        assert!(text.contains("<item objectid=\"2\" transform=\"1 0 0 0 1 0 0 0 1 0 0 0\"/>"), "{text}");
        // X goes to +Y, Y to -X, Z stays; then the origin.
        assert!(text.contains("<item objectid=\"2\" transform=\"0 1 0 -1 0 0 0 0 1 5 6 7\"/>"), "{text}");
    }

    #[test]
    fn what_cannot_be_a_model_is_refused() {
        assert!(write(&Model::default()).unwrap_err().contains("no solid"));
        let mut m = part();
        m.objects[0].mesh.triangles.clear();
        assert!(write(&m).unwrap_err().contains("no surface"));
        let mut m = part();
        m.objects[0].mesh.positions[1].x = f64::NAN;
        assert!(write(&m).unwrap_err().contains("not a number"));
        let mut m = part();
        m.items[0].object = 3;
        assert!(write(&m).unwrap_err().contains("not there"));
    }
}
