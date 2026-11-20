# MisterDiagram rendering regression baseline

Captured from the macOS arm64 GitHub Actions build of commit `510822872b7a72f43b0b01903a4f31bb2f057532`. The [CI run](https://github.com/bmurdock/bjm-MisterDiagram/actions/runs/44632987024) produced the binary, and its artifact SHA256 was verified before use. The build uses the bundled fonts. This is a baseline of MisterDiagram output. Independent Visio compatibility remains unmeasured.

| Fixture | Coverage |
| --- | --- |
| Office network | The repository's contributor-authored `tools/examples/office-network.sh` example covers native masters, connected routing, and labels. |
| Multipage | Two foreground pages with different sizes share a background. They exercise patterned fill, accented text, bold and italic text, and bundled font substitution. The authoring recipe is in `multipage/commands.json`. |

The VSDX files are the inputs for reruns. Both folders include the PDF, its rendered pages at 72 dpi, and the extracted text and font inventory. PNG uses the CLI's native preview canvas. SVG and PDF use the physical page canvas. `baseline.json` records checksums, dimensions, SVG font families, and PDF verification details. All eight primary exports reproduced byte for byte from the saved inputs on the same build.

## Check a candidate build

Use Python 3.11+, Pillow 10+, and Poppler's `pdfinfo`, `pdffonts`, `pdftotext`, and `pdftoppm` on PATH. From the repository root, build with the same bundled fonts used for the baseline and run the comparison. The Linux CI job runs this check against its release binary.

```sh
CRAFT_FONTS_DIR='' cargo build --locked --release -p misterdiagram-cli
python3 tools/render-regression/check.py --cli target/release/misterdiagram
```

| Acceptance criterion | Evidence |
| --- | --- |
| Determinism | The original build reproduced all eight primary exports byte for byte. Add `--strict-bytes` to check exact bytes against that capture on a controlled platform. |
| Regression stability | The runner compares PNG dimensions and decoded RGBA pixels, SVG element order, attributes and text, and PDF page counts, dimensions, per-page text, font names, embedding, subsetting, and Unicode mappings. It also compares each PDF page at 72 dpi using the same Poppler installation for both files. |
| Visio compatibility | Independent comparison in licensed Visio is pending. The private oracle and its acceptance criteria are described in [development.md](../../docs/development.md). |

The runner checks the recorded reference hashes before rendering into a temporary directory. A difference reports the fixture, export, and failed property and exits with a failure. For an intentional rendering change, capture the affected outputs separately and submit the reference files, updated manifest entries, and a before/after explanation together for review. Hashes identify the captured files. Build provenance and reproducibility require separate evidence.

## Font resolution in previews

The SVGs declare the requested family first, for example `'Calibri', 'Carlito'`. A browser can select an installed Calibri, while the fixture PDF embeds Carlito. The SVG comparison checks the declaration. For visual verification in the future Mac editor, use the engine-rendered PNG with bundled fonts, or explicitly map the SVG families to the same bundled faces before using the browser preview to judge text layout.

## Reproduce an individual export

To inspect a difference, run the corresponding command from this directory with the candidate binary on PATH and a fresh output directory.

```bash
mkdir -p candidate/office-network candidate/multipage
misterdiagram convert office-network/office-network.vsdx candidate/office-network/page-1.png
misterdiagram render office-network/office-network.vsdx --page 1 candidate/office-network/page-1.svg
misterdiagram render office-network/office-network.vsdx candidate/office-network/document.pdf
misterdiagram render multipage/multipage.vsdx candidate/multipage/document.pdf
for page in 1 2; do
  misterdiagram render multipage/multipage.vsdx --page "$page" "candidate/multipage/page-$page.png"
  misterdiagram render multipage/multipage.vsdx --page "$page" "candidate/multipage/page-$page.svg"
done
```
