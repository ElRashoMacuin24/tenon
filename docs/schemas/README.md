# Command schemas

[commands.json](commands.json) says what every Tenon command takes, as JSON Schema: the names of
its parameters, their types, which must be given and which go together. It is for programs
that drive Tenon (scripts, agents through the [MCP server](../mcp.md)) and for the people who
write them. [commands.md](../commands.md) says the same in words.

```sh
tenon-cli commands --schema                                  # print it
tenon-cli commands --schema --out docs/schemas/commands.json # write the file in the repository
```

The file is generated from the command registry. A test fails when it is out of date
(`every_command_has_a_schema_and_the_published_document_is_the_registrys`), so a change to any
command's parameters shows in review as a change to this file.

## The document

```json
{
  "title": "Tenon commands",
  "version": 1,
  "$defs": { "face_ref": { ... }, "edge_ref": { ... } },
  "commands": [
    {
      "id": "model.extrude",
      "label": "Extrude",
      "undoable": true,
      "description": "sketch; distance (plus backward: ...) ...",
      "params": {
        "type": "object",
        "properties": {
          "sketch": { "type": "integer", "minimum": 0, "description": "the sketch: its feature id, ..." },
          "distance": { "type": "number", "x-unit": "mm", "description": "how far" },
          "operation": { "type": "string", "enum": ["join", "cut", "new_body", "intersect"], "description": "..." }
        },
        "additionalProperties": false,
        "required": ["sketch"]
      }
    }
  ]
}
```

- `params` is a JSON Schema (draft 2020-12) of the command's parameters. `$ref`s point into the
  document's `$defs`.
- `additionalProperties` is false everywhere: a parameter a command does not have is an error,
  told with the name that was probably meant.
- `x-unit` says what a number measures: `mm` or `rad`. A number without it is a plain number (a
  count, a factor), or says its unit in its description (a density in g/cm³, an overhang in
  degrees).
- `x-path` marks a file or folder. Relative paths are taken from the working folder (for
  `tenon-cli run`, its `--out` folder).
- Where one of several parameters must be given, each is optional and `allOf` says the choice:
  `{"anyOf": [{"required": ["start"]}, {"required": ["x1", "y1"]}]}` is "`start`, or `x1` and
  `y1`".
- An optional parameter given as `null` counts as not given.
- Words (`enum`) are matched whatever their case.
- A face or edge reference (`face_ref`, `edge_ref`) is an object that `model.face_ref`,
  `model.edge_ref` and `model.find` return. Its inside is Tenon's business and not part of the
  schema: pass it back as it came.

Some parameters are only said to be an object (`{"type": "object"}`) with the rest in their
description: a sketch constraint, a feature's whole definition (`feature.add`), a target of an
assembly relationship, a pick on a drawing. Their shapes are in [commands.md](../commands.md)
and [file-format.md](../file-format.md).

## What is checked, and when

- **Commands that come from outside are checked before they run, in every build**: a script
  (`tenon-cli run`), the MCP server, `batch.run`. A parameter that is not one of the command's,
  a missing one, or one of the wrong type is refused by name, and nothing is changed.
- **In development every command is checked, both ways.** In builds with debug assertions, which
  is every test run, each command that runs is held to its schema: what it is *given* must be
  declared, and what it *reads* must be declared (commands read their parameters through a
  function that notes the names). So a schema that leaves a parameter out fails whichever test
  runs the command. When version 1 was published every command was run by at least one test
  (found by logging every command the test suite ran).
- The app's own calls are not checked in a release build: they are the app's, and are covered
  by the tests.

## Versions

`version` is one number for all the schemas.

**Within a version the schemas only grow**: new commands, new optional parameters, more words in
an `enum`, wider types. A program written against version 1 keeps working against any later
version 1.

**Anything that could break such a program needs a new version**: a command or a parameter
removed or renamed, an optional parameter made required, a type or an `enum` that takes less,
a change in which parameters must be given together. `tenon-cli commands --schema --out FILE`
refuses to write over a file of the same version that the new one would break, and says what
breaks; the version is raised in `crates/model/src/schema.rs` and the change is listed here.

What a command *returns* is not in the schemas yet. Results are described in words in
[commands.md](../commands.md); they follow the same rule in practice (fields are added, not
taken away), without a check.

### Changes

- **1** (2026-10-12): the first version. 204 commands.
  - Since, without a new version (only more is taken): the twenty `study.*` commands and the
    optional `youngs_modulus`, `poisson` and `yield_strength` of `document.material` (M11).
    Of them, `study.fix` and `study.force` at first required `faces`; they now take `faces`
    or `edges`, so what was valid still is. Then `document.properties` (a part's number and
    description, for 0.1.0); after it `sketch.text` and `sketch.edit_text` (DEC-102), the optional
    `consumed` and `dependents` of `feature.delete` (DEC-103) and `to` of `model.extrude`
    (DEC-104); `model.hole`'s spotface type, `spotface_diameter`, `spotface_depth` and `to`
    (DEC-105); `feature.visibility` (DEC-106); `backward` and `reverse` of `model.revolve` (DEC-108); `to_next` of `model.extrude`
    (DEC-109); `asm.constrain`'s tangent type and `inside`, and `inside` of
    `asm.edit_relationship` (DEC-110); `from` of `model.extrude` (DEC-111); `to` and `to_next`
    of `model.revolve` (DEC-112); `tapped`, `thread_length`, `pitch`, `designation` and `left` of
    `model.hole` (DEC-114). 228 commands.
