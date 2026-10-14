Tenon {version}
===========

Tenon is free, open-source parametric 3D CAD: sketch, model parts, sheet metal, assemblies,
drawings, 3D printing checks and quick stress checks. https://github.com/ElRashoMacuin24/tenon


Starting it
-----------

Nothing needs installing: everything Tenon needs is in this folder. Keep the folder together.

Windows   Double-click tenon.exe. Windows may say it protected your PC from an unrecognised
          app (Tenon is not signed): choose "More info", then "Run anyway".

macOS     Move Tenon.app to Applications if you like, then Control-click it and choose Open
          (Tenon is not notarised by Apple, so the first start needs that once). If macOS
          says the app is damaged, run this once in Terminal:
              xattr -dr com.apple.quarantine /Applications/Tenon.app

Linux     Run bin/tenon. A graphics driver with Vulkan or OpenGL 3.3 is needed.

Open a file from the examples folder to look around, or start a new part from the File menu.


The command line
----------------

tenon-cli (beside tenon on Windows, in bin on Linux, inside Tenon.app/Contents/MacOS on
macOS) runs command scripts, converts and inspects files, renders pictures and serves
agents over the Model Context Protocol. Run `tenon-cli` with no arguments to see how.


Stress checks
-------------

Working a stress study out takes two free programs that are not part of Tenon: Gmsh (the
mesher, https://gmsh.info) and CalculiX (the solver, http://www.dhondt.de). Install both and
put them on your search path, or tell Tenon where they are in Tools > Application Options.
Everything else in Tenon works without them.

A quick check is an engineering aid, not a certified analysis.


Where things are kept
---------------------

Your files are where you save them. Tenon keeps its settings and the copies it makes to
recover unsaved work in your user folder (Windows: %APPDATA%\Tenon; macOS:
~/Library/Application Support/Tenon; Linux: ~/.local/share/tenon).


Licences
--------

Tenon is MIT OR Apache-2.0 (LICENSE-MIT, LICENSE-APACHE, NOTICE). It is built on
OpenCASCADE Technology (LGPL-2.1 with the Open CASCADE exception), whose libraries are in
this folder as separate files, and on the other software listed in
licenses/THIRD-PARTY.txt, each with its licence. CHANGELOG.md says what is new.
