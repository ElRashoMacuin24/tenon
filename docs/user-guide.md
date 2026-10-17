# Getting started with Tenon

Tenon is free, open-source parametric 3D CAD: you sketch, constrain, build features, assemble
parts, and document them in drawings. This page is for people who use Tenon. To build it from
its source, see [setup.md](setup.md).

## Installing

Download the archive for your system from the
[Releases](https://github.com/ElRashoMacuin24/tenon/releases) page and unpack it anywhere.
Nothing else needs installing: the folder holds Tenon and every library it needs. Keep the
folder together.

| System | Start | The first time |
|---|---|---|
| Windows 10 or 11 (64-bit) | `tenon.exe` | Windows may say it protected your PC from an unrecognised app: choose **More info**, then **Run anyway**. Tenon is not signed. |
| macOS 11 or later (Apple silicon) | `Tenon.app` | Control-click it and choose **Open** (Tenon is not notarised). If macOS says the app is damaged, run `xattr -dr com.apple.quarantine Tenon.app` once in Terminal. |
| Linux (64-bit, glibc 2.35 or later) | `bin/tenon` | A graphics driver with Vulkan or OpenGL 3.3. |

The archive also holds example parts, assemblies and drawings (`examples`), the drawing
templates (`templates`), and `tenon-cli`, the command-line program.

Intel Macs and other systems: build Tenon from its source ([setup.md](setup.md)).

## The window

- **The ribbon** across the top holds the commands of what you are working on, in tabs:
  3D Model, Sketch, Sheet Metal, Inspect, Tools, Manage, View, Environments for a part;
  Assemble for an assembly; Place Views and Annotate for a drawing. Greyed buttons are
  commands Tenon does not have yet; their tip says so.
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
6. Click the top face, **Start 2D Sketch**, draw a circle, **Finish Sketch**, then **Hole**
   (H) or **Extrude** with Cut.
7. **Fillet** (F): click edges, type the radius, Enter.
8. **File > Save.** A part is one `.tenon` file, plain text.

Change anything later: double-click the feature in the browser, or the sketch under it. What
came after it is rebuilt, and the faces and edges later features used are found again.

**Manage > Parameters** (fx) gives sizes names; any value can be a formula of them.
**Manage > Design Table** keeps one part in several sizes. **Manage > Standard Parts** starts
a part from a screw, nut, washer, bearing or section in one of its sizes.

## Sketching

Lines, arcs, circles, rectangles, slots (straight or along an arc), polygons and splines; Trim, Extend, Split, Offset,
Move, Copy, Rotate, Scale, Mirror and patterns. Constraints are found as you draw
(horizontal, perpendicular, tangent...) and can be added from the Constrain panel; the
status bar says how many degrees of freedom are left, and curves that are fully
constrained change colour. **Project Geometry** brings edges of the part into the
sketch. A dimension can be typed as a formula of parameters.

**Text** (Sketch > Create): click where the text starts and type it in the Format Text box,
with the height of its capital letters and an angle; Enter starts a new line, OK places it.
Double-click a text to change it, or drag it by its box. Its letters are profiles: extrude
a sketch that holds only text to raise it, or cut it into a face to engrave it. When a sketch
also has closed curves, click inside the text's box in the Extrude panel to add its letters.
Text uses one font, DejaVu Sans, built in so that a part looks the same on every computer.

**Points** (Sketch > Create > Point) are centre points, drawn as a cross: a Hole goes at
every one. **Center Point** (Sketch > Format) makes the selected points sketch points, drawn
as a dot, which a hole takes only when you pick them, or centre points again; with nothing
selected it switches what Point makes.

## Features

An extrusion goes a **Distance**, **Through All**, **To Next** (as far as the part's next face;
a cut goes through the material it starts in), **Between** two faces or planes, or **To** a
flat face or plane: choose To
under Extents, then click the face, a work plane, or a plane in the browser.

To delete a feature, right-click it in the browser and choose **Delete**. When sketches it is
made from, or features that use it, would be left, the Delete Features box lists them; they
go with it unless you untick them.

Extrude, Revolve, Sweep, Loft, Coil, Rib; Hole (simple, counterbored, countersunk), Thread,
Fillet, Chamfer, Shell, Draft, Split, Combine; rectangular and circular patterns, Mirror;
work planes, axes and points; Box, Cylinder, Sphere and Torus. Every one opens its panel
again when its row is double-clicked.

**Inspect > Properties** gives the part its number and description (for parts lists) and
its material (its colour, density and how strong it is), and says its mass, volume, centre
of gravity and moments of inertia. **Inspect > Measure** measures
distances, angles, lengths and areas.

## Sheet metal

**File > New Sheet Metal Part**, or **3D Model > Convert to Sheet Metal**. Set the
thickness, bend radius, reliefs and K factor in **Sheet Metal Defaults**. Then Face,
Flange, Contour Flange, Hem, Fold, Bend, Corner Seam, Cut (also across bends) and Corner
Round/Chamfer. **Go to Flat Pattern** lays it flat; **File > Export Flat Pattern DXF**
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

An assembly file refers to its parts by where they are relative to it: move them together.

## Drawings

**File > New Drawing** (ANSI or ISO), or **New Drawing from Template**. **Base View** shows a
part or an assembly; then projected, section, detail and isometric views. Dimensions follow
the model; hole tables, parts lists, balloons and text are on Annotate. **Export PDF, SVG or
DXF** from the File menu.

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

Tenon 0.1 is a first release. Among what it does not do: sub-assemblies, motion studies and
parts that adapt to one another; shaded drawing views, tolerances and ordinate dimensions;
ellipses in sketches, a choice of fonts for sketch text; Lofted Flange, Contour Roll, Punch, Rip and Unfold in sheet metal;
stress studies of assemblies or of more than one material. [ROADMAP.md](../ROADMAP.md)
lists everything that works, with the test that shows it, and what is missing.
