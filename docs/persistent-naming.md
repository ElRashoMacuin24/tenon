# Persistent naming

Features must refer to faces and edges ("fillet these three edges", "sketch on this face") in a way
that survives edits upstream: a changed sketch dimension, an added hole, a reordered feature.
Kernel indices (`FaceId { shape, index }`) do not survive any of that and are never stored.

**Status:**

- **M0:** the kernel half (operation history, below) and its tests.
- **M1:** faces of extrusions and revolutions are named and resolved.
- **M2:** edges; faces made by fillets, chamfers, shells, holes, ribs and pattern or mirror
  copies; splits by geometry; the required tests (end of this page). Vertices are not referenced.

The sections below say what is implemented; "Reference format" onwards is the original design,
kept for the parts not built (neighbour origins, split ordinals, vertex references).

## What is implemented

`crates/model/src/naming.rs`. Every body keeps, for each face, a `FaceOrigin`:

- `Cap { feature, end: start | end }`: the start or end cap of an extrusion or partial
  revolution, from the kernel's `StartCap` / `EndCap` roles.
- `Side { feature, curve }`: the face swept from sketch curve `curve`, from the profile tags.
- `From { feature, source, ordinal }`: a face made by `feature` from a named sub-shape whose key
  is `source` (a stable FNV-1a hash):
  - fillets and chamfers: from the edge (key of its two face names) or the corner vertex (key of
    the faces around it); a shell's inner walls from the face they offset (`names_of_modify`);
  - holes: from the centre point and the segment of the hole's cross-section (`HoleFace`:
    wall, bottom, drill point, counterbore wall and floor, countersink);
  - ribs: from the rib line;
  - pattern and mirror copies: from the copied face's name, `ordinal` = copy number.

Booleans pass names on through `images` (`names_of_boolean`); modify operations through images
and `generated` (`names_of_modify`).

A `FaceRef` is an origin plus a `Fingerprint` (surface kind, normal or axis, centroid, area).
An `EdgeRef` is the names of the two faces the edge joins (sorted) plus an `EdgeFingerprint`
(midpoint, length). Sketches on faces, fillets, chamfers, shells, patterns, mirrors and work
features store them.

Resolving takes the faces with the same origin (for edges: the edges between the same two named
faces) and, if there are several, the nearest fingerprint. A face split in two has its origin
twice, and the fingerprint picks the piece. If nothing has the origin, the feature fails with a
message naming the reference ("the referenced edge no longer exists (the edge between the side
face of Extrusion1 from sketch curve e6 and ...)"); the features after it are not computed, the
part is shown as it stood before the failing feature, and the document is unchanged.

Not built: neighbour origins and the "clear margin" check; `Split` ordinals (fingerprints pick
instead); vertex references; logging of history gaps (a test checks that there are none for the
M2 features instead).
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
   that feature with an error naming the reference ("Fillet3: edge between Extrusion1 side face (L7)
   and Extrusion1 end cap no longer exists"). The last good result stays visible, and the user
   re-picks. Features after it are not computed.

Origin match is preferred over geometry, so moving a face (a changed dimension) still resolves.
Geometry only breaks ties, so a face that changes type, such as a plane becoming a cylinder, is
reported as broken rather than silently mismatched.

## Required tests and their proof

Model tests in `crates/model/tests/m2.rs` unless named otherwise. The corpus in
`crates/model/tests/naming_corpus.rs` (M5) runs eleven upstream edits on one part holding an edge
reference (a fillet) and a face reference (a sketch on a face). For each edit it says where both
must land, or that the part must break with a message naming what is gone:
- a dimension made larger or smaller, a taller extrusion, an extrusion turned the other way;
- a hole added to the first sketch, an independent cut added before the fillet;
- the referenced face cut in two;
- two features reordered, an unrelated feature suppressed;
- the line a referenced face came from deleted and drawn again;
- the feature the references come from deleted (refused, naming its users).

| Required | Test |
|---|---|
| An early sketch dimension changes; downstream fillets and chamfers still resolve: lengthen | `fillets_follow_upstream_edits` |
| ... shorten | `edge_references_survive_shortening_and_added_sketch_geometry` |
| ... flip a dimension sign | the corpus (`crates/model/tests/naming_corpus.rs`): an extrusion turned the other way; the references follow the face they name to the other side |
| A sketch entity added upstream leaves references alone | `edge_references_survive_shortening_and_added_sketch_geometry` |
| The entity a reference depends on is deleted: broken, with a clear message | `a_lost_edge_breaks_the_fillet_with_a_clear_message` |
| A face cut in two keeps references to each piece | `a_split_face_keeps_each_piece_by_geometry` (by fingerprint) |
| Two independent features reordered; references still resolve | `references_survive_reordering_independent_features` |
| Every result face of every operation gets an origin (no history gaps) | `every_face_of_m2_features_is_named` (fillets, countersunk holes, a pattern, a shell; a fixed set of models, not randomised) |
| Faces of the M1 features | `bracket_with_holes_regenerates_after_an_upstream_edit`, `sketch_dimension_edit_moves_the_hole`, `a_lost_face_reference_breaks_the_feature_clearly`, `extrude_then_cut_keeps_tags_through_the_boolean` |
| Edges of holes and of pattern copies | `a_hole_follows_its_point_and_keeps_its_edges`, `rectangular_pattern_of_a_hole_follows_the_hole` |