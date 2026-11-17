# MisterDiagram agent guidance

These rules apply throughout this repository. Follow the current user request and the applicable
global instructions when they are more specific. Use [CONTRIBUTING.md](CONTRIBUTING.md) for the
contributor contract and [docs/development.md](docs/development.md) for build and oracle gates.

## 1. Assets and provenance

### 1.1 Keep vendor product assets out

- Do not commit, embed, publish, trace or closely imitate visual assets from Microsoft, Adobe,
  Avid or Autodesk products, including their installed fonts, icons, themes, templates and sample
  drawings. This restriction also applies to their openly licensed visual designs.
- Visio-produced files, shipped stencils, COM dumps and Visio screenshots belong only in the
  gitignored local/private corpus. Never include them in repository files, documentation or
  distributable artifacts. See [docs/provenance.md](docs/provenance.md).
- Use contributor-original assets or assets with a proven permitted licence. Record source,
  version, licence and use in [ATTRIBUTION.md](ATTRIBUTION.md), retain required notices, and run
  `cargo xtask assets` when an asset changes. If provenance cannot be established, leave it out.

## 2. Clean-room implementation

- Use published format references and black-box observations on synthetic drawings. Do not read
  another Visio implementation's source or copy its code or vendor artwork.
- Cite the public reference for new format elements, cells and functions. Record use of
  non-Microsoft Visio documentation in `docs/provenance.md` section 3.
- Keep the private oracle corpus private. A PR without that corpus can claim reference coverage;
  only a maintainer with the corpus can confirm measured Visio parity.

## 3. Hostile input and failure handling

- Treat packages, XML, images, formulas, MCP messages and command parameters as untrusted. Bound
  allocation, recursion and output; use checked arithmetic and return errors instead of panics.
- Preserve the workspace's `unsafe_code = "forbid"` rule and production lint restrictions on
  `unwrap`, `expect`, `panic`, `unreachable`, `todo` and `unimplemented`.
- Add a focused hostile-input regression for a new parser path or a crash fix. Preserve session
  state and written files on rejected or failed operations as documented in [docs/mcp.md](docs/mcp.md).

## 4. Architecture

Follow the crate layers and dependency boundaries in [docs/architecture.md](docs/architecture.md).
User-visible authoring actions go through engine commands; the MCP and CLI surfaces share that
engine. Do not add a UI dependency to the headless core.

## 5. Validation

- Run the narrowest relevant checks first. `cargo xtask ci` is the repository gate for formatting,
  Clippy, workspace tests, rendering, assets, layers, sources and wasm. Report any baseline or
  environment failure rather than weakening the gate to make a change appear green.
- Corpus-dependent measurements require the private local corpus and the prerequisites in
  `docs/development.md`. A skipped corpus check is not measured parity.

## 6. Local inputs and releases

- `.cargo/config.toml` points local development builds at an optional sibling `craft-fonts`
  checkout. Treat it as read-only; do not write to sibling repositories. Release builds set
  `CRAFT_FONTS_DIR` empty so only the attributed bundled faces are embedded.
- Follow [docs/release.md](docs/release.md) for upstream release gates. The workflow creates a
  draft from a version tag; publication requires the maintainer's release decision. Do not infer
  authorization to commit, push, tag or publish from historical project notes.

## 7. Operating rules

- Read available local `plan/STATUS.md`, `plan/execution-plan.md` and `log/devlog.md` at the start
  of a project session. They are gitignored local state; use tracked docs when they are absent.
- Never edit an oracle output, threshold, floor, denominator or volatile list merely to pass a
  test. If new evidence warrants a change, record the source, old and new coverage, and rationale
  in the relevant parity/gap document and use the project's human acceptance gate.
- When Visio disagrees with the implementation, preserve the witness and investigate against the
  public reference before changing code or acceptance criteria. Keep decisions and remaining gaps
  in the existing planning documents without publishing private corpus material.
