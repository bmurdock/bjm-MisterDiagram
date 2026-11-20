# MisterDiagram rendering regression baseline

Captured from the macOS arm64 GitHub Actions build of commit `510822872b7a72f43b0b01903a4f31bb2f057532`. The [CI run](https://github.com/bmurdock/bjm-MisterDiagram/actions/runs/44632987024) produced the binary, and its artifact SHA256 was verified before use. The build uses the bundled fonts. This is a baseline of MisterDiagram output. Independent Visio compatibility remains unmeasured.

| Fixture | Coverage |
| --- | --- |
| Office network | The repository's contributor-authored `tools/examples/office-network.sh` example covers native masters, connected routing, and labels. |
| Multipage | Two foreground pages with different sizes share a background. They exercise patterned fill, accented text, bold and italic text, and bundled font substitution. The authoring recipe is in `multipage/commands.json`. |

The VSDX files are the inputs for reruns. Both folders include the PDF, its rendered pages at 72 dpi, and the extracted text and font inventory. PNG uses the CLI's native preview canvas. SVG and PDF use the physical page canvas. `baseline.json` records checksums, dimensions, SVG font families, and PDF verification details. All eight primary exports reproduced byte for byte from the saved inputs on the same build.

## Reproduce on a candidate build

Run these from this directory with a candidate `misterdiagram` binary on `PATH` and a clean output directory. Compare the outputs with the matching reference files and use the PDF text, fonts, and rasterized pages to investigate any changes.

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

A hash mismatch signals a change for review. Compare decoded PNG pixels, SVG dimensions and text, and PDF page images and extracted text before attributing it to rendering.
