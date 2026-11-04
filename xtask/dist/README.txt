Tenon {version}
===========

Tenon is free, open-source parametric 3D CAD: sketch, model parts, sheet metal, assemblies,
drawings, 3D printing checks, quick stress checks and shapes optimized for a stress study's
loads. https://github.com/ElRashoMacuin24/tenon

Getting started: docs/user-guide.md in this folder. docs also says how scripts, the commands,
agents, stress checks and Optimize Shape work, and what is in a Tenon file.


Starting it
-----------

Nothing needs installing: everything Tenon needs is in this folder. Keep the folder together.

Windows   Double-click tenon.exe. Windows may say it protected your PC from an unrecognised
          app (Tenon is not signed): choose "More info", then "Run anyway".

macOS     Move Tenon.app to Applications if you like. Tenon is not notarised by Apple, so the
          first start takes a step more. On macOS 15 and later: try to open it, then choose
          Open Anyway in System Settings > Privacy & Security. On earlier versions: Control-click
          it and choose Open. If macOS still will not open it, or says it is damaged, run this
          once in Terminal, with the path of the app where it is:
              xattr -dr com.apple.quarantine Tenon.app

Linux     Run bin/tenon. A graphics driver with Vulkan or OpenGL 3.3 is needed.

Open a file from the examples folder to look around, or start a new part from the File menu.
The examples' READMEs are written for a checkout of the sources: what they run with
`pixi run cargo run -p tenon-cli -- run x.json` is `tenon-cli run x.json --out DIR` here, and
the pictures and exported files they name are not in this folder.


The command line
----------------

tenon-cli (beside tenon on Windows, in bin on Linux, inside Tenon.app/Contents/MacOS on
macOS) runs command scripts, converts and inspects files, renders pictures and serves
agents over the Model Context Protocol. Run `tenon-cli` with no arguments to see how.


Stress checks
-------------

Working a stress study out, and Optimize Shape, take two free programs that are not part of
Tenon: Gmsh (the mesher, https://gmsh.info) and CalculiX (the solver, http://www.dhondt.de).
Install both (docs/stress.md says how) and put them on your search path, or tell Tenon where
they are in Tools > Application Options. tenon-cli takes them from the search path, or from
where TENON_GMSH and TENON_CCX say. Everything else in Tenon works without them.

A quick check is an engineering aid, not a certified analysis. What Optimize Shape works out is
a guide to model over, not a finished part: check what you make with a stress study.


Where things are kept
---------------------

Your files are where you save them. Tenon keeps its settings and the copies it makes to
recover unsaved work in your user folder (Windows: %APPDATA%\Tenon; macOS:
~/Library/Application Support/Tenon; Linux: ~/.local/share/tenon), and the shapes of the parts
you have opened, so that they open quickly, in your cache folder (Windows:
%LOCALAPPDATA%\Tenon\cache; macOS: ~/Library/Caches/Tenon; Linux: ~/.cache/tenon). That
folder can be emptied whenever Tenon is not running.


Licences
--------

Tenon is MIT OR Apache-2.0 (LICENSE-MIT, LICENSE-APACHE, NOTICE). It is built on
OpenCASCADE Technology (LGPL-2.1 with the Open CASCADE exception), whose libraries are in
this folder as separate files, and on the other software listed in
licenses/THIRD-PARTY.txt, each with its licence. CHANGELOG.md says what is new.
