//! Dependency layering rules (docs/architecture.md, "Layering").
//!
//! The rule engine works on a small, metadata-independent model so it can be
//! unit-tested; `from_metadata` builds that model from `cargo metadata`.

use serde_json::Value;

/// Where a workspace crate sits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    /// Regular layered crate.
    Layer(u8),
    /// Layer 0, and additionally may depend on no workspace crate at all.
    Standalone,
    /// A kernel backend (the only home of FFI and `unsafe`). It sits at the given layer for its own
    /// dependencies; layered crates may use it only as a dev-dependency, so everything above
    /// `kernel` stays backend-neutral. Apps pick the backend.
    Backend(u8),
    /// Test tooling: may depend on anything below the UI (it sits at L9 for rule
    /// purposes); other crates may use it only as a dev-dependency.
    Testkit,
    /// Binaries and build tooling: exempt from the rules.
    Exempt,
}

impl Class {
    fn layer(self) -> Option<u8> {
        match self {
            Class::Layer(l) | Class::Backend(l) => Some(l),
            Class::Standalone => Some(0),
            Class::Testkit => Some(9),
            Class::Exempt => None,
        }
    }
}

/// The layering table. Names are package names without the `tenon-` prefix.
pub const TABLE: &[(&str, Class)] = &[
    ("geom", Class::Layer(0)),
    ("dxf", Class::Standalone),
    ("kernel", Class::Layer(1)),
    ("kernel-occt", Class::Backend(2)),
    ("sketch", Class::Layer(2)),
    ("model", Class::Layer(3)),
    ("assembly", Class::Layer(4)),
    ("drawing", Class::Layer(5)),
    ("io", Class::Layer(6)),
    ("render", Class::Layer(7)),
    ("ui", Class::Layer(8)),
    ("testkit", Class::Testkit),
    // apps and tooling
    ("tenon", Class::Exempt),
    ("cli", Class::Exempt),
    ("xtask", Class::Exempt),
];

/// Explicit orderings *within* a layer (earlier may be used by later). Empty today.
pub const INTRA_LAYER_ORDER: &[&[&str]] = &[];

fn intra_layer_allowed(from: &str, to: &str) -> bool {
    let (from, to) = (short_name(from), short_name(to));
    INTRA_LAYER_ORDER.iter().any(|chain| match (chain.iter().position(|n| *n == from), chain.iter().position(|n| *n == to)) {
        (Some(f), Some(t)) => t < f,
        _ => false,
    })
}

/// External crates that constitute a UI toolkit / windowing dependency.
/// Entries ending in `*` are prefixes.
pub const UI_CRATES: &[&str] = &["egui", "eframe", "egui-wgpu", "egui_extras", "egui_kittest", "winit", "rfd", "bevy*"];
/// First layer allowed to use UI crates (`ui`).
pub const UI_MIN_LAYER: u8 = 8;

/// GPU API crates: the renderer may use them without knowing about the UI toolkit.
pub const GPU_CRATES: &[&str] = &["wgpu"];
/// First layer allowed to use GPU crates (`render`).
pub const GPU_MIN_LAYER: u8 = 7;

/// Native-binding crates: only a kernel backend may use them (normal or build dependency).
pub const NATIVE_CRATES: &[&str] = &["cxx", "cxx-build", "cc", "bindgen", "opencascade*", "occt*"];

pub fn short_name(pkg: &str) -> &str {
    pkg.strip_prefix("tenon-").unwrap_or(pkg)
}

pub fn classify(pkg: &str) -> Option<Class> {
    let s = short_name(pkg);
    TABLE.iter().find(|(n, _)| *n == s).map(|(_, c)| *c)
}

fn matches_any(list: &[&str], name: &str) -> bool {
    list.iter().any(|p| match p.strip_suffix('*') {
        Some(prefix) => name.starts_with(prefix),
        None => name == *p,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DepKind {
    Normal,
    Dev,
    Build,
}

#[derive(Debug, Clone)]
pub struct Dep {
    pub name: String,
    pub kind: DepKind,
    /// `true` if the dependency is a workspace member.
    pub workspace: bool,
}

#[derive(Debug, Clone)]
pub struct Crate {
    pub name: String,
    pub deps: Vec<Dep>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Violation {
    Unregistered { krate: String },
    Upward { krate: String, dep: String, from: u8, to: u8, kind: DepKind },
    StandaloneHasWorkspaceDep { krate: String, dep: String },
    TestkitAsNormalDep { krate: String },
    BackendAsNormalDep { krate: String, dep: String },
    ToolkitTooLow { krate: String, dep: String, layer: u8, min: u8 },
    NativeOutsideBackend { krate: String, dep: String },
}

impl std::fmt::Display for Violation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Violation::Unregistered { krate } => {
                write!(f, "{krate}: unknown workspace crate; register it in xtask/src/layers.rs TABLE (see docs/architecture.md)")
            }
            Violation::Upward { krate, dep, from, to, kind } => {
                write!(f, "{krate} (L{from}) -> {dep} (L{to}) [{kind:?}]: may only depend on strictly lower layers")
            }
            Violation::StandaloneHasWorkspaceDep { krate, dep } => {
                write!(f, "{krate}: standalone crate must not depend on workspace crate {dep}")
            }
            Violation::TestkitAsNormalDep { krate } => {
                write!(f, "{krate}: tenon-testkit may only be a dev-dependency")
            }
            Violation::BackendAsNormalDep { krate, dep } => {
                write!(f, "{krate} -> {dep}: kernel backends may only be dev-dependencies of library crates; go through `dyn Kernel`")
            }
            Violation::ToolkitTooLow { krate, dep, layer, min } => {
                write!(f, "{krate} (L{layer}) depends on `{dep}`, which is only allowed from L{min} up")
            }
            Violation::NativeOutsideBackend { krate, dep } => {
                write!(f, "{krate} depends on native-binding crate `{dep}`; FFI belongs in a kernel backend crate only")
            }
        }
    }
}

/// Check all rules. Returns violations sorted for stable output.
pub fn check(crates: &[Crate]) -> Vec<Violation> {
    let mut out = Vec::new();
    for c in crates {
        let Some(class) = classify(&c.name) else {
            out.push(Violation::Unregistered { krate: c.name.clone() });
            continue;
        };
        if class == Class::Exempt {
            continue;
        }
        let layer = class.layer().unwrap_or(0);
        for d in &c.deps {
            // Self dev-dependencies (e.g. to enable features in tests) are fine.
            if d.name == c.name {
                continue;
            }
            if d.workspace {
                if class == Class::Standalone {
                    out.push(Violation::StandaloneHasWorkspaceDep { krate: c.name.clone(), dep: d.name.clone() });
                    continue;
                }
                match classify(&d.name) {
                    // Unregistered deps are reported on their own entry.
                    None => {}
                    Some(Class::Testkit) => {
                        if d.kind != DepKind::Dev {
                            out.push(Violation::TestkitAsNormalDep { krate: c.name.clone() });
                        }
                    }
                    Some(Class::Backend(_)) if !matches!(class, Class::Testkit) => {
                        if d.kind != DepKind::Dev {
                            out.push(Violation::BackendAsNormalDep { krate: c.name.clone(), dep: d.name.clone() });
                        }
                    }
                    Some(dc) => {
                        let to = dc.layer().unwrap_or(u8::MAX);
                        if to >= layer && !(to == layer && intra_layer_allowed(&c.name, &d.name)) {
                            out.push(Violation::Upward {
                                krate: c.name.clone(),
                                dep: d.name.clone(),
                                from: layer,
                                to: if to == u8::MAX { 10 } else { to },
                                kind: d.kind,
                            });
                        }
                    }
                }
            } else {
                if layer < UI_MIN_LAYER && matches_any(UI_CRATES, &d.name) {
                    out.push(Violation::ToolkitTooLow { krate: c.name.clone(), dep: d.name.clone(), layer, min: UI_MIN_LAYER });
                }
                if layer < GPU_MIN_LAYER && matches_any(GPU_CRATES, &d.name) {
                    out.push(Violation::ToolkitTooLow { krate: c.name.clone(), dep: d.name.clone(), layer, min: GPU_MIN_LAYER });
                }
                if !matches!(class, Class::Backend(_)) && matches_any(NATIVE_CRATES, &d.name) {
                    out.push(Violation::NativeOutsideBackend { krate: c.name.clone(), dep: d.name.clone() });
                }
            }
        }
    }
    out.sort_by_key(|v| v.to_string());
    out.dedup();
    out
}

/// Build the model from `cargo metadata --format-version 1 --no-deps`.
pub fn from_metadata(meta: &Value) -> Result<Vec<Crate>, String> {
    let pkgs = meta["packages"].as_array().ok_or("metadata: no packages array")?;
    let members: Vec<&str> = pkgs.iter().filter_map(|p| p["name"].as_str()).collect();
    let mut out = Vec::new();
    for p in pkgs {
        let name = p["name"].as_str().ok_or("package without name")?.to_owned();
        let mut deps = Vec::new();
        for d in p["dependencies"].as_array().into_iter().flatten() {
            let dname = d["name"].as_str().unwrap_or_default().to_owned();
            let kind = match d["kind"].as_str() {
                Some("dev") => DepKind::Dev,
                Some("build") => DepKind::Build,
                _ => DepKind::Normal,
            };
            let workspace = members.contains(&dname.as_str()) || d["path"].is_string();
            deps.push(Dep { name: dname, kind, workspace });
        }
        out.push(Crate { name, deps });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

pub fn describe(class: Option<Class>) -> String {
    match class {
        Some(Class::Layer(l)) => format!("L{l}"),
        Some(Class::Standalone) => "L0 standalone".into(),
        Some(Class::Backend(l)) => format!("L{l} backend"),
        Some(Class::Testkit) => "testkit".into(),
        Some(Class::Exempt) => "exempt".into(),
        None => "UNREGISTERED".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(name: &str, deps: &[(&str, DepKind, bool)]) -> Crate {
        Crate { name: name.into(), deps: deps.iter().map(|(n, k, w)| Dep { name: (*n).into(), kind: *k, workspace: *w }).collect() }
    }
    use DepKind::*;

    #[test]
    fn clean_downward_graph_passes() {
        let g = [
            c("tenon-geom", &[("robust", Normal, false)]),
            c("tenon-kernel", &[("tenon-geom", Normal, true)]),
            c("tenon-model", &[("tenon-kernel", Normal, true), ("tenon-kernel-occt", Dev, true), ("tenon-testkit", Dev, true)]),
            c("tenon-render", &[("tenon-kernel", Normal, true), ("wgpu", Normal, false)]),
            c("tenon-ui", &[("tenon-render", Normal, true), ("egui", Normal, false)]),
            c("tenon-cli", &[("tenon-kernel-occt", Normal, true)]),
        ];
        assert!(check(&g).is_empty(), "{:?}", check(&g));
    }

    #[test]
    fn upward_dependency_flagged() {
        let v = check(&[c("tenon-kernel", &[("tenon-model", Normal, true)])]);
        assert!(matches!(v[..], [Violation::Upward { from: 1, to: 3, .. }]));
    }

    #[test]
    fn drawing_may_not_use_io() {
        let v = check(&[c("tenon-drawing", &[("tenon-drawing", Dev, true), ("tenon-io", Normal, true)])]);
        assert!(matches!(v[..], [Violation::Upward { from: 5, to: 6, .. }]));
    }

    #[test]
    fn backend_only_as_dev_dependency_of_libraries() {
        let v = check(&[c("tenon-model", &[("tenon-kernel-occt", Normal, true)])]);
        assert!(matches!(v[..], [Violation::BackendAsNormalDep { .. }]), "{v:?}");
        assert!(check(&[c("tenon-model", &[("tenon-kernel-occt", Dev, true)])]).is_empty());
        assert!(check(&[c("tenon", &[("tenon-kernel-occt", Normal, true)])]).is_empty());
    }

    #[test]
    fn backend_may_not_reach_above_kernel() {
        let v = check(&[c("tenon-kernel-occt", &[("tenon-model", Normal, true)])]);
        assert!(matches!(v[..], [Violation::Upward { from: 2, to: 3, .. }]), "{v:?}");
        assert!(check(&[c("tenon-kernel-occt", &[("tenon-kernel", Normal, true), ("cxx", Normal, false), ("cxx-build", Build, false)])]).is_empty());
    }

    #[test]
    fn native_crates_only_in_backend() {
        for dep in ["cxx", "cxx-build", "cc", "opencascade-sys", "occt-sys"] {
            let v = check(&[c("tenon-model", &[(dep, Normal, false)])]);
            assert!(matches!(v[..], [Violation::NativeOutsideBackend { .. }]), "{dep}: {v:?}");
        }
    }

    #[test]
    fn toolkits_below_their_layer_flagged() {
        for dep in ["egui", "eframe", "winit", "egui_kittest", "rfd", "bevy_ecs"] {
            let v = check(&[c("tenon-render", &[(dep, Normal, false)])]);
            assert!(matches!(v[..], [Violation::ToolkitTooLow { layer: 7, min: 8, .. }]), "{dep}");
        }
        let v = check(&[c("tenon-io", &[("wgpu", Normal, false)])]);
        assert!(matches!(v[..], [Violation::ToolkitTooLow { layer: 6, min: 7, .. }]), "{v:?}");
        assert!(check(&[c("tenon-render", &[("wgpu", Normal, false)])]).is_empty());
        assert!(check(&[c("tenon-ui", &[("winit", Normal, false)])]).is_empty());
    }

    #[test]
    fn self_dev_dependency_ignored() {
        assert!(check(&[c("tenon-model", &[("tenon-model", Dev, true)])]).is_empty());
    }

    #[test]
    fn upward_dev_dependency_flagged() {
        let v = check(&[c("tenon-geom", &[("tenon-kernel", Dev, true)])]);
        assert!(matches!(v[..], [Violation::Upward { kind: Dev, .. }]));
    }

    #[test]
    fn unregistered_crate_is_error() {
        let v = check(&[c("tenon-mystery", &[])]);
        assert!(matches!(&v[..], [Violation::Unregistered { krate }] if krate == "tenon-mystery"));
        assert!(v[0].to_string().contains("register"));
    }

    #[test]
    fn standalone_has_no_workspace_deps() {
        let v = check(&[c("tenon-dxf", &[("tenon-geom", Normal, true)])]);
        assert!(matches!(v[..], [Violation::StandaloneHasWorkspaceDep { .. }]));
    }

    #[test]
    fn testkit_only_as_dev_dependency() {
        let v = check(&[c("tenon-render", &[("tenon-testkit", Normal, true)])]);
        assert!(matches!(v[..], [Violation::TestkitAsNormalDep { .. }]));
        assert!(check(&[c("tenon-render", &[("tenon-testkit", Dev, true)])]).is_empty());
        assert!(check(&[c("tenon-testkit", &[("tenon-model", Normal, true), ("tenon-kernel-occt", Normal, true)])]).is_empty());
    }

    #[test]
    fn apps_and_xtask_exempt() {
        for app in ["tenon", "tenon-cli", "xtask"] {
            assert!(check(&[c(app, &[("egui", Normal, false), ("tenon-ui", Normal, true)])]).is_empty());
        }
    }

    #[test]
    fn metadata_parsing() {
        let meta: Value = serde_json::from_str(
            r#"{"packages":[
                {"name":"tenon-kernel","dependencies":[
                    {"name":"tenon-geom","kind":null,"path":"/x/crates/geom"},
                    {"name":"serde","kind":null},
                    {"name":"proptest","kind":"dev"}]},
                {"name":"tenon-geom","dependencies":[]}
            ]}"#,
        )
        .unwrap();
        let g = from_metadata(&meta).unwrap();
        assert_eq!(g.len(), 2);
        let k = g.iter().find(|c| c.name == "tenon-kernel").unwrap();
        assert!(k.deps[0].workspace && !k.deps[1].workspace);
        assert_eq!(k.deps[2].kind, Dev);
        assert!(check(&g).is_empty());
    }
}
