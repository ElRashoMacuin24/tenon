# Getting started with Tenon

Tenon is free, open-source parametric 3D CAD: you sketch, constrain, build features, assemble
parts, and document them in drawings. This page is for people who use Tenon. To build it from
its source, see [setup.md](https://github.com/ElRashoMacuin24/tenon/blob/main/docs/setup.md).

## Installing

Download the archive for your system from the
[Releases](https://github.com/ElRashoMacuin24/tenon/releases) page and unpack it anywhere.
Nothing else needs installing: the folder holds Tenon and every library it needs. Keep the
folder together.

| System | Start | The first time |
|---|---|---|
| Windows 10 or 11 (64-bit) | `tenon.exe` | Windows may say it protected your PC from an unrecognised app: choose **More info**, then **Run anyway**. Tenon is not signed. |
| macOS 11 or later (Apple silicon) | `Tenon.app` | Tenon is not notarised. On macOS 15 and later, try to open it, then choose **Open Anyway** in System Settings > Privacy & Security. On earlier versions, Control-click it and choose **Open**. If macOS still will not open it, or says it is damaged, run `xattr -dr com.apple.quarantine Tenon.app` once in Terminal, with the path of the app where it is. |
| Linux (64-bit, glibc 2.35 or later) | `bin/tenon` | A graphics driver with Vulkan or OpenGL 3.3. |

The archive also holds example parts, assemblies and drawings (`examples`), the drawing
templates (`templates`), this guide and the other documents for users (`docs`), and `tenon-cli`,
the command-line program.

Intel Macs and other systems: build Tenon from its source ([setup.md](https://github.com/ElRashoMacuin24/tenon/blob/main/docs/setup.md)).

## The window

- **The ribbon** across the top holds the commands of what you are working on, in tabs:
  3D Model, Sketch, Inspect, Tools, Manage, View, Environments for a part (Sheet Metal and Flat
  Pattern on a sheet metal part, Stress Analysis while you are in it); Assemble for an assembly;
  Place Views and Annotate for a drawing. Greyed buttons are commands Tenon does not have yet;
  their tip says so. In a window narrower than the ribbon, an arrow at its end goes along it.
- **Search commands** (top right) finds any command by name.
- **The browser** on the left lists the part's features in order (or the assembly's
  components, or the drawing's sheets and views). Double-click a feature to change it;
  right-click it for more. Drag **End of Part** up to see the part as it was.
- **The properties panel** opens on the left while a command is being set up: fill it in, then
  **OK** (or Enter). **Cancel** (or Esc) leaves without a change. **+** makes this one and
  starts the next.
- **The status bar** at the bottom says what the command wants next, and what went wrong
  when something did.

### Moving round the model

| To | Do |
|---|---|
| Turn the view | Shift + middle mouse drag, or hold F4 and drag |
| Pan | Middle mouse drag, or hold F2 and drag |
| Zoom | The mouse wheel (about the pointer), or hold F3 and drag |
| Look along a side | Click a face or an edge of the orientation cube (top right) |
| The whole part, from three-quarters | F6 (Home View) |
| The view before | F5 |

Right-click in the view for the **radial menu**: the commands you are most likely to want
next, by a flick of the mouse.

### Keys

| Key | Command |
|---|---|
| Ctrl+N, Ctrl+O, Ctrl+S | New part, Open, Save (Cmd on macOS) |
| Ctrl+Z, Ctrl+Y (or Ctrl+Shift+Z) | Undo, Redo |
| S | Start 2D Sketch |
| E, R | Extrude, Revolve |
| H, F | Hole, Fillet |
| L, C, A | Line, Circle, Arc (in a sketch) |
| D | Dimension (in a sketch, or a drawing) |
| X, O | Trim, Offset (in a sketch) |
| F8, F9 | Show, hide constraints (in a sketch) |
| P, J, C | Place, Joint, Constrain (in an assembly) |
| T, B | Text, Balloon (in a drawing) |
| Esc | Leave the command (and stop a stress study that is being worked out) |

## Your first part

1. **File > New Part.**
2. **Start 2D Sketch** (S), then click the XY plane in the view.
3. **Rectangle**: click one corner, then the other. Type a size as you draw, or afterwards
   use **Dimension** (D): click an edge, then where the dimension goes, and type its value.
4. **Finish Sketch.**
5. **Extrude** (E): the profile is chosen; type the distance and press Enter.
6. A hole: press **Hole** (H) and click the top face where it goes, then OK. Or a cut of any
   shape: click the top face, **Start 2D Sketch**, draw a circle, **Finish Sketch**, then
   **Extrude** with Cut.
7. **Fillet** (F): click edges, type the radius, Enter.
8. **File > Save.** A part is one `.tenon` file, plain text.

Change anything later: double-click the feature in the browser, or the sketch under it. What
came after it is rebuilt, and the faces and edges later features used are found again.

**Manage > Parameters** (fx) gives sizes names; any value can be a formula of them.
**Manage > Design Table** keeps one part in several sizes. **Manage > Standard Parts** starts
a part from a screw, nut, washer, bearing or section in one of its sizes.

## Sketching

Lines, arcs, circles, ellipses, rectangles, slots (straight or along an arc), polygons and splines; Trim, Extend, Split, Offset,
Move, Copy, Rotate, Scale, Stretch, Mirror and patterns. Constraints are found as you draw
(horizontal, perpendicular, tangent...) and can be added from the Constrain panel, where
**Smooth** joins a spline to the line, arc or spline it ends on with the same bend on both sides; the
status bar says how many degrees of freedom are left, and curves that are fully
constrained change colour. **Project Geometry** brings edges of the part into the
sketch. A dimension can be typed as a formula of parameters.

**Text** (Sketch > Create): click where the text starts and type it in the Format Text box,
with the height of its capital letters and an angle; Enter starts a new line, OK places it.
Double-click a text to change it, or drag it by its box. Its letters are profiles: extrude
a sketch that holds only text to raise it, or cut it into a face to engrave it. When a sketch
also has closed curves, click inside the text's box in the Extrude panel to add its letters
(beside those curves, not over them: click the curves' region to take it away).
Text uses one font, DejaVu Sans, built in so that a part looks the same on every computer.

**Points** (Sketch > Create > Point) are centre points, drawn as a cross: a Hole goes at
every one. **Center Point** (Sketch > Format) makes the selected points sketch points, drawn
as a dot, which a hole takes only when you pick them, or centre points again; with nothing
selected it switches what Point makes.

### 3D sketches

**Start 3D Sketch** (under the arrow of Start 2D Sketch) opens a sketch on no plane, with a 3D Sketch tab in
place of the Sketch tab. It is a feature in the browser; double-click its row to go back into it, and Finish
3D Sketch to leave it. Undo works in it as in any sketch. It is drawn in a colour of its own.

- **Line** (L). Click the points, or type X, Y and Z in the box at the top of the view and press Add or
  Enter. A click on a vertex of the part, a work point or the origin ties the line's end to it: when the part
  changes, it goes along. A click on a point of the sketch joins the lines there. A click on a face is on
  the face. A click on nothing is in the plane through the last point that faces you, so orbit first to
  choose how a line leaves. Near an axis through the last point, the line stays on that axis and is held to it
  (the rubber band takes the axis's colour). Esc ends the chain.
- **Spline**. Click or type the points it goes through. Enter in the box puts the point typed; Enter after that,
  or Make Spline, makes the spline.
- **Bend**. Click the corner where two lines meet and say the radius. The corner stays as a faint point: what
  held it still holds it, and delete the bend (its arc, its centre or its corner) to have the corner back. A radius
  too large for its lines is refused with the largest that fits, when it is first said and if it is changed.
- **Point**. A point on its own: on a vertex or a work point it follows the part.
- **Include Geometry**. Click edges of the part: they come in, in amber, and follow the part. Break Link makes
  a straight one or a point the sketch's own.

Constraints are on the tab too: Coincident, Fix, Parallel to X, Y or Z, Parallel, Perpendicular, Equal and
Tangent (two curves that meet at an end). **Dimension** takes a line's length, a bend's radius or the distance
between two points, and X, Y and Z Coordinate where a point is; double-click a value to change it, click it and
press Delete to remove it, and a dimension is a parameter like a sketch's. The status bar says how many degrees
of freedom are left, and what is held whole changes colour. What would over-constrain the sketch is refused, and
the status bar says why and what is in the way. Drag a point to move it, in the plane that faces you; one that
follows the part or is fixed stays, and says so. A curve selected says its length in the status bar; Measure
(Inspect) takes a click on a curve too, and Shift adds up several. An Equal on a bent line measures it to the
corner, as a length that was on it before the bend does.

**Sweep** takes a 3D sketch as its path: from its list, which takes all the sketch's lines, bends and splines
and the edges brought in that join them, or a click on a curve, which takes the run of curves it is in. With
nothing of its own, the edges brought in are the path. Make an edge construction to keep it out. A bend made or
deleted after the sweep is taken in by it, and so is a line drawn on from the end of a path that was all the
sketch had. Round a corner with no bend it is mitred, between two straight lines, if the profile is one outline
with no holes; at a spline or an arc it says so, and to make the two tangent or put a line and a bend between. A
spline closed on itself has a corner where it closes. The profile goes on a plane across the path's start. **Work
Plane > Square to a 3D Sketch Curve** makes that plane: click the curve near its end. **Work Point > At a Point**
takes a point of the sketch, a work axis a line of it, and a three-point plane its points or vertices of the part.

## Features

An extrusion goes a **Distance**, **Through All**, **To Next** (as far as the part's next face;
a cut goes through the material it starts in), **Between** two faces or planes, or **To** a
flat face or plane: choose To
under Extents, then click the face, a work plane, or a plane in the browser.

To delete a feature, right-click it in the browser and choose **Delete**. When sketches it is
made from, or features that use it, would be left, the Delete Features box lists them; they
go with it unless you untick them.

A revolution turns **Default**, **Flipped**, **Symmetric** or **Asymmetric** (two angles), or goes
**To** a face or plane through the axis, or **To Next**.

A **Hole** goes at the centre points of a sketch, or with no sketch of centres you click a flat
face where each goes. Its type is Simple, Clearance (for a screw to pass) or **Tapped** (a screw's
tap drill, with the thread on its wall, full depth or a depth; drawings note it). Its seat is a
counterbore, a **spotface** or a countersink. It goes a depth, through all, or **To** a face or plane.

**Work planes** go by an offset, an angle, the middle of two planes or **three points**. A
sketch's or work feature's menu in the browser has **Visibility**, which shows or hides it.

Extrude, Revolve, Sweep, Loft, Coil, Rib; Hole, Thread,
Fillet, Chamfer, Shell, Draft, Split, Combine; rectangular and circular patterns, Mirror;
work planes, axes and points; Emboss, Derive and UCS; Box, Cylinder, Sphere and Torus. Every one opens its panel
again when its row is double-clicked. A fillet can be Variable (a radius at each end of its
edges, marked in the view), and **Face Fillet** (under Fillet) rounds every edge between two
sets of faces.

**Emboss** puts a closed profile from a sketch on the part, its text too. Draw it first: the panel
opens on the last sketch nothing has been made from yet, and its list has the others. **Raise from Face** and
**Recess into Face** go on what the profile is seen on along the sketch's normal, by the depth along
each face's normal, so that on a shaft the top follows the round; Direction says which way the
faces look. Only what can be seen from the sketch takes it: a profile wider than a shaft stops at
the shaft's sides, and a floor under an overhang takes nothing where the overhang covers it. It
goes on one solid, the last that the profile falls on, even if the profile is seen on another as
well, and even if another solid lies over it: solids are judged one at a time; a profile drawn in open space, in a hole's middle say, is seen from there. A profile that
falls on nothing, or a cut that would leave a piece of the part loose, is refused in words.

**From Sketch Plane** adds material on one side of the plane and takes it away on the other, each as
far as the depth: the shape ends on the plane. Direction changes the two sides over. Keep the plane
off the part's surface: on the surface one way changes nothing, and the other would leave what is
added standing loose on the rim of what is taken away; a plane inside the part, flipped, would shut
a hollow in under what is added. All of these are refused in words, and the part stays as it was.

A taper leans the sides so that the shape comes out of a mould. It is hinged at the sketch plane, so
with the plane away from the faces the foot of what is raised is wider than the profile, the more
the further off it is: for the profile's own size at the foot, draw the sketch on the face. Tick
**Wrap to Face** and click a cylinder or a cone to lay the profile round it with its lengths kept: a
word as long as the round it covers. It stays on that face: what runs past the face's edge is left
off, and a profile wholly off the face is refused. It reads as the sketch does from the side its
normal looks to, so face the sketch outward. On a cone a straight line stays straight as the cone
would be laid out flat, so a long line of text bends down at its ends.

**Derive** brings in the solid of another part file: choose the file, then a scale and a mirror
plane if it needs them, joined to the part or as a new solid. Nothing of that file is copied into
this one: its path is kept relative to this file, so a folder of parts can move together. When the
other file has changed, **Manage > Rebuild All** (or opening the part again) takes the change;
saving the part does not, under this name or another. If the file is gone the feature says so, the
part still opens, and a double-click on its row lets you choose the file again.

**UCS** (Work Features) is a coordinate system of your own: typed offsets and turns from the
part's origin or from another UCS, or Three Points clicked on the part (its origin, a point on its
X, a point on its Y side). It is drawn as a triad, and its row in the browser opens like the Origin
folder. Each of its planes, axes and its origin can be clicked in the view or the browser wherever
work planes, axes and points are taken: to start a sketch, for Extrude To, for a mirror plane or
a pattern's direction, and to measure to. The triad keeps a size you can see and click whatever the
size of the part. Measure says where one of its axes or planes is, and which way it goes, before a
second pick gives the distance and the angle.

**Inspect > Properties** gives the part its number and description (for parts lists) and
its material (its colour, density and how strong it is), and says its mass, volume, centre
of gravity and moments of inertia. **Inspect > Measure** measures
distances, angles, lengths and areas, and to work features.

## Sheet metal

**File > New Sheet Metal Part**, or **3D Model > Convert to Sheet Metal**. Set the
thickness, bend radius, reliefs and K factor in **Sheet Metal Defaults**. Then Face,
Flange, Contour Flange, Hem, Fold, Bend, Corner Seam, Cut (also across bends) and Corner
Round/Chamfer. **Create Flat Pattern** lays it flat (**Go to Flat Pattern** afterwards); **File > Export Flat Pattern DXF**
writes it for a cutter.

## Assemblies

**File > New Assembly**, then **Place** (P) part files: the first is grounded at the
origin; then each instance follows the pointer, a click puts it down and the next follows,
until Esc or a right-click. Hold them with **Constrain** (C: mate, with its Mate and Flush
solutions, angle, tangent, insert, symmetry) or **Joint** (J: rigid, rotational, slider, cylindrical, planar,
ball). Drag a component: its relationships hold.
Double-click a component to change its part in place. **Inspect > Analyze Interference**
finds parts that overlap; **Manage > Bill of Materials** lists them (and exports CSV).
**Standard Parts** places screws, nuts, washers, bearings and sections in a chosen size.

**Pattern** repeats components: click them, then an edge, flat face or axis for the direction (the
axes and planes under Origin in the browser serve too), and give a count and a spacing, a second
direction if there are to be rows of rows; or Circular, a round face or edge for the axis, a count
and the angle they spread over. The view shows them before OK. In the browser it is one row with
its elements under it: double-click it to change it, and elements move, come and go; its menu has
Suppress, Make Elements Independent and Delete. An element is a component like any other to the
parts list, a constraint or a drawing, but it stands where the pattern puts it: move what it
repeats. Suppress takes out its elements and what a later pattern makes of them; one you hid on its
own stays hidden when they come back. If what gives a pattern its direction goes from its part, its
row turns red, the elements stay where they are, and Update and the status line say so: double-click
the row to pick again.

**Mirror** reflects components in a flat face or a plane. Each is a mirrored occurrence of the same
part file, with a row of its own in the parts list as a mirrored part; it follows its source, the
plane, and any change to the part, and can be constrained to as the image of what it mirrors: a
block mated to its top stands on it. For a part that is the same as its mirror image, choose Placed
again: the same part, in the mirrored position, on the same row of the list.

**Copy** makes independent copies, of the same parts or (A new part file) each in a new part file
beside the assembly. They go beside what they are copied from: type how far instead, or click
Where and put them down with a click. Constraints among what is copied come too.

An assembly file refers to its parts by where they are relative to it: move them together.

## Drawings

**File > New Drawing** (ANSI or ISO), or **New Drawing from Template**. **Base** shows a
part or an assembly; then projected, section, detail and isometric views. Dimensions follow
the model; hole tables, parts lists, balloons and text are on Annotate. **Export PDF, SVG or
DXF** from the File menu.

- **Auxiliary** (Place Views > Create): click a view, a straight edge of it, then where the new
  view goes. It looks square to that edge, from the side it is placed on, stays lined up with its
  parent and turns with the edge when the model changes. Double-click its row for its side
  or another edge.
- **Overlay** (Place Views > Create): click a view of an assembly and type a position: the values of
  the joints and offsets that change, blank being as the assembly has it. What moves shows on the
  sheet as you type, and what cannot be reached is said in the panel. OK draws the assembly in that
  position over the view in phantom lines.
- **Crop, Break, Break Out** (Place Views > Modify): click a view, then click what the panel asks
  for on it, or type it, and OK. Crop draws only what lies inside a rectangle or a circle. Break
  takes out the band between two lines, up the sheet or across, and leaves a gap; dimensions keep
  their true values, and a gap wider than the band at the view's scale leaves the band as wide as
  it is. Break Out cuts a rectangle or a circle of the view away down to a depth from what is
  nearest you, or through the part, and hatches the floor of the cut; if that takes nothing away,
  the status bar and the view's row say so. A centre mark or a hole's tag goes with what it marks.
  Each has a row under its view: double-click it to change it, right-click it to suppress or
  delete it. A panel closes when you start another tool or go to another sheet.
**Baseline** and **Ordinate** (Annotate > Dimension) measure several places from one. Click the
datum or origin (an edge, a circle for its centre; near a line's end, that end), then what to
measure. A second click on one takes it away. The set shows where it would go; click clear of
the edges to put it there: above or below the view it measures across the sheet, to the left or
right it measures up the sheet, unless the panel says otherwise.

- A baseline set has a dimension from the datum to each place, the shortest nearest the view
  and the rest evenly beyond it (Spacing in the panel; wider where text needs it).
- An ordinate set marks the origin 0 and each place with its position from it, on its own line
  with no arrows, negative to the left of the origin or below it. Values that would touch are
  moved apart and their lines jog to reach them.

A set is one thing: drag it to move it, Delete to delete it, double-click it or its row in the
browser to add to it, take from it or change it. Its values follow the model.

All its places are in one view, on one sheet. A click in another view, or with another sheet
showing, is refused and what you have picked stays. Isometric views, and views made from them, are
not dimensioned. Press OK without clicking where it goes and it goes outside the view, on the side
where the whole of it fits; if neither side has room you are told, and a click puts it where you
want. Choosing the other direction in the panel moves it outside the view too, unless you have
clicked where it goes.

## 3D printing

**Environments > 3D Print Check** checks the part for a printer: a closed surface, walls
too thin, overhangs, and whether it fits the bed, printed on the side you choose.
**File > Export STL or 3MF.**

## Stress checks

**Environments > Stress Analysis.** Create Study; say where the part is held (Fixed, Pin,
Frictionless) and what loads it (Force, Pressure, Gravity, Bearing, Moment, Remote Force,
Body) by clicking its faces; Assign a material that says how strong it is; **Simulate**.
The results are drawn in colour on the part.

Working a study out takes two free programs that are not part of Tenon: Gmsh and CalculiX.
Install them ([stress.md](stress.md) says how) and, if they are not on your search path,
tell Tenon where they are in **Tools > Application Options**. A quick check is an
engineering aid, not a certified analysis: [stress.md](stress.md) says what it does and
does not know.

**Optimize Shape** (the Analysis tab's Optimize panel) works out where material is needed to
carry the study's loads, for a share of the mass kept: Keep Faces for bolt holes and mating
faces, Symmetry to tick the origin planes or work planes the shape is the same on both sides
of, then Optimize Shape. The shape is drawn over the part; a slider shows other shares; Export
Shape writes it as STL or 3MF to model over. It takes minutes; Esc stops it and keeps the shape
so far (with a menu, a panel or a prompt open, Esc is that one's first), and leaving the environment
or changing the part stops it and drops what it came to. What there is to know of the shape is
written over the view. Simulate's results hide the shape, and its row in the browser brings it
back. [topology.md](topology.md) says how it works and what it cannot do.

## Files

| File | What |
|---|---|
| `.tenon` | a part |
| `.tenonasm` | an assembly: where its parts are, and how they are held together |
| `.tenondrw` | a drawing: its sheets and views, and which files they show |

All three are plain text ([file-format.md](file-format.md)): they can be kept in Git and
compared (`tenon-cli diff a.tenon b.tenon`). STEP, STL and 3MF are written for other
programs; STEP files can be inspected and converted with `tenon-cli`.

### If Tenon closes unexpectedly

Tenon keeps a copy of unsaved work every half minute. When it starts after it did not close
properly, it offers that work back. If the cause was a fault in Tenon itself, a report of
what failed is written, and the next start says where it is: please send it with an issue at
[github.com/ElRashoMacuin24/tenon/issues](https://github.com/ElRashoMacuin24/tenon/issues).

Tenon keeps its settings, the copies of unsaved work and the fault reports in your user
folder: `%APPDATA%\Tenon\data` on Windows, `~/Library/Application Support/Tenon` on macOS,
`~/.local/share/tenon` on Linux.

## The command line, scripts and agents

`tenon-cli` runs command scripts ([scripts.md](scripts.md)), renders pictures, converts and
inspects STEP files, compares Tenon files, and serves AI agents over the Model Context
Protocol ([mcp.md](mcp.md)). Every command the ribbon runs is listed in
[commands.md](commands.md).

## What Tenon does not do yet

Among what Tenon does not do yet: sub-assemblies, motion studies and parts that adapt to one
another; shaded drawing views, tolerances and ordinate dimensions; a choice of fonts for sketch
text, and letters outside Latin, Greek and Cyrillic (in the window such a name shows as boxes,
and in sketch text it is cut as boxes, though the file opens and works); a taper on an ellipse, a
spline or a letter's curves; Lofted Flange, Contour Roll, Punch, Rip and Unfold in sheet metal;
stress studies of assemblies or of more than one material. A window holds one document: File >
Open replaces it, and a second window is the way to have two open. [ROADMAP.md](https://github.com/ElRashoMacuin24/tenon/blob/main/ROADMAP.md)
Tenon 0.1 is a first release. Among what it does not do: sub-assemblies, motion studies and
parts that adapt to one another; shaded drawing views and tolerances;
ellipses in sketches, a choice of fonts for sketch text; Lofted Flange, Contour Roll, Punch, Rip and Unfold in sheet metal;
stress studies of assemblies or of more than one material. [ROADMAP.md](../ROADMAP.md)
lists everything that works, with the test that shows it, and what is missing.
