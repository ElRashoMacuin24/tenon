# Persistent naming

Features must refer to faces and edges ("fillet these three edges", "sketch on this face") in a way
that survives edits upstream: a changed sketch dimension, an added hole, a reordered feature.
Kernel indices (`FaceId { shape, index }`) do not survive any of that and are never stored.

**Status:**

- **M0:** the kernel half (operation history, below) and its tests.
- **M1:** faces of extrusions and revolutions are named and resolved (next section).
- **M2:** edges, vertices, splits, adjacency, and the full test list at the end.

This document is the design all three implement.

## What M1 implements

`crates/model/src/naming.rs`. Every body keeps, for each face, a `FaceOrigin`:

- `Cap { feature, end: start | end }`: the start or end cap of an extrusion or partial
  revolution, from the kernel's `StartCap` / `EndCap` roles;
- `Side { feature, curve }`: the face swept from sketch curve `curve`, from the profile tags.

Booleans pass names on through `images` (`names_of_boolean`). A `FaceRef` is an origin plus a
`Fingerprint` (surface kind, normal or axis, centroid, area). It is stored by sketches on faces
(`PlaneRef::Face`) and in project files.

Resolving takes the faces with the same origin and, if there are several, the nearest
fingerprint. If no face has the origin, the feature fails with a message naming the reference
("the referenced face no longer exists (end face of Extrude1)"). The rest of the tree is not
computed, and the document is unchanged.

Tests:

- `bracket_with_holes_regenerates_after_an_upstream_edit` and `sketch_dimension_edit_moves_the_hole`:
  an upstream dimension change moves a face, and the sketch on it follows.
- `a_lost_face_reference_breaks_the_feature_clearly`.
- `extrude_then_cut_keeps_tags_through_the_boolean`.
- The M1 demo script, which edits the base thickness under a sketch on the base's top face.

Gaps until M2:

- edges and vertices;
- `Split` ordinals (a face cut in two gets the same origin twice, and the fingerprint picks);
- neighbour origins, and the "clear margin" check on fingerprints;
- `FromSubShape` origins for fillets and chamfers;
- logging of history gaps.

## What the kernel provides (M0, done)

Every modelling operation returns `Op { shape, history }` (`crates/kernel/src/history.rs`):

- `images`: for every face and edge of every input, the result sub-shapes it became. One means
  kept or modified; several means split; none means deleted.
- `generated`: result sub-shapes created by the operation, keyed by what created them:
  - an input sub-shape, such as a fillet face from an edge or a section edge from a face;
  - a tagged profile curve, such as an extrude side face from sketch line #7.
- `roles`: for primitives, which face is the box's +Z face, a cylinder's lateral face, and so on.

Tests: `crates/kernel-occt/tests/m0.rs`
- `box_face_roles_match_geometry`: each role's face has the right normal.
- `cut_through_hole_with_history`:
  - the plate's top face maps to exactly one result face with the hole's area removed;
  - an untouched side face keeps its area;
  - the tool's caps are reported deleted and its wall becomes the hole.

Profiles carry caller tags (`TaggedCurve2::tag`), normally sketch entity ids. That is how extrude
and revolve side faces are traced back to sketch geometry (M1).

## Reference format (M2)

A feature stores a `TopoRef`:

```text
TopoRef {
    kind: Face | Edge | Vertex,
    origin: Origin,          // how it was created (below)
    fingerprint: Fingerprint,
    neighbours: Vec<Origin>, // origins of adjacent faces (for edges: the two faces)
}

Origin =
    FeatureRole { feature: FeatureId, role: Role }             // e.g. Extrude#3 EndCap
  | FromCurve   { feature: FeatureId, curve: SketchEntityId }  // Extrude#3 side face from line L7
  | FromSubShape{ feature: FeatureId, parent: Box<Origin> }    // Fillet#5 face from edge (...)
  | Split       { parent: Box<Origin>, ordinal: u32 }           // the n-th piece, ordered spatially

Fingerprint { surface kind, normal or axis, radius, area, centroid, bounding box }
```

Ids are stable model-level ids (`FeatureId`, `SketchEntityId`), never kernel indices.

## Naming during regeneration

Regeneration walks the feature list. After each kernel operation the model updates a map from
result sub-shape to `Origin`, using the operation's history:

1. Sub-shapes listed in `images` inherit their input's origin. A split gets `Split` with an
   ordinal from a deterministic spatial order: sort by centroid along the feature's main direction,
   then the other axes.
2. Sub-shapes listed in `generated` get `FromCurve` / `FromSubShape` / `FeatureRole` origins.
3. Anything left unnamed is a kernel history gap. It gets a fingerprint-only name and is logged,
   so that history coverage can be tested.

## Resolving a reference

To find the sub-shape a `TopoRef` means in the newly regenerated body:

1. **Origin match.** Take the candidates whose origin equals the reference's origin. One
   candidate: done.
2. **Adjacency.** Several candidates (for example after a split): keep those whose neighbour
   origins overlap the stored neighbours most.
3. **Fingerprint.** Still several: pick the nearest fingerprint, with the same surface kind
   required. Accept it only if it is closer than any other by a clear margin (tolerances from
   `tenon_geom::tol`).
4. **Failure.** No candidate, or still ambiguous: the feature is **broken**. Regeneration stops at
   that feature with an error naming the reference ("Fillet3: edge between Extrude1 side face (L7)
   and Extrude1 end cap no longer exists"). The last good result stays visible, and the user
   re-picks. Features after it are not computed.

Origin match is preferred over geometry, so moving a face (a changed dimension) still resolves.
Geometry only breaks ties, so a face that changes type, such as a plane becoming a cylinder, is
reported as broken rather than silently mismatched.

## Tests required before M2 is "done"

- Edit an early sketch dimension and check that downstream fillets and chamfers still resolve to
  the same logical edges. Cases: lengthen, shorten, flip a dimension sign.
- Add a sketch entity upstream and check that existing references are unaffected.
- Delete the entity a reference depends on and check that the feature reports broken, with a
  clear message, and that the previous result stays.
- Split cases: a cut that divides a face into two keeps references to each piece stable across
  regenerations.
- Reorder two independent features and check that references still resolve.
- A coverage check: for every operation in randomised models, every result face and edge gets an
  origin from history (no gaps).
