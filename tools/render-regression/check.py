"""Compare a bundled-font MisterDiagram build with the checked-in render baseline."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import xml.etree.ElementTree as ET

from PIL import Image, ImageChops

BASE = Path(__file__).resolve().parent


def run(*args):
    result = subprocess.run(args, capture_output=True, env={**os.environ, "LC_ALL": "C"})
    if result.returncode:
        raise ValueError(f"{Path(args[0]).name} exited {result.returncode}: "
                         f"{result.stderr.decode(errors='replace').strip()}")
    return result.stdout.decode("utf-8")


def equal(label, expected, actual):
    if expected != actual:
        if isinstance(expected, str) and isinstance(actual, str):
            offset = next((i for i, pair in enumerate(zip(expected, actual))
                           if pair[0] != pair[1]), min(len(expected), len(actual)))
            label = f"{label} at character {offset + 1}"
            expected, actual = expected[offset:offset + 80], actual[offset:offset + 80]
        raise ValueError(f"{label}: expected {str(expected)[:180]!r}, got {str(actual)[:180]!r}")


def sha256(path):
    with path.open("rb") as data:
        return hashlib.file_digest(data, "sha256").hexdigest()


def compare_pixels(reference, candidate):
    with Image.open(reference) as before, Image.open(candidate) as after:
        equal("pixel dimensions", before.size, after.size)
        diff = ImageChops.difference(before.convert("RGBA"), after.convert("RGBA"))
        bounds = diff.getbbox(alpha_only=False)
        if bounds:
            raise ValueError(f"RGBA pixels differ in box {bounds} (left, top, right, bottom)")


def compare_svg(expected, actual, location="svg", in_text=False):
    equal(f"{location} element", expected.tag, actual.tag)
    for key in sorted(expected.attrib.keys() | actual.attrib.keys()):
        equal(f"{location} @{key}", expected.get(key), actual.get(key))
    # Indentation between drawing elements is immaterial. Whitespace inside text is visible.
    in_text = in_text or expected.tag.rsplit("}", 1)[-1] in {"text", "tspan"}
    equal(f"{location} text", expected.text if in_text else (expected.text or "").strip(),
          actual.text if in_text else (actual.text or "").strip())
    equal(f"{location} child count", len(expected), len(actual))
    for index, (left, right) in enumerate(zip(expected, actual), 1):
        child = f"{location}/{left.tag.rsplit('}', 1)[-1]}[{index}]"
        compare_svg(left, right, child, in_text)
        equal(f"{child} following text", left.tail if in_text else (left.tail or "").strip(),
              right.tail if in_text else (right.tail or "").strip())


def check_pdf(reference, candidate, pages, fonts):
    info = run("pdfinfo", "-f", "1", "-l", str(len(pages)), candidate)
    count = re.search(r"^Pages:\s+(\d+)$", info, re.MULTILINE)
    equal("PDF page count", len(pages), int(count[1]) if count else None)
    sizes = re.findall(r"^Page\s+\d+ size:\s+([\d.]+) x ([\d.]+) pts", info, re.MULTILINE)
    equal("PDF page sizes (pt)", [tuple(map(float, page)) for page in pages],
          [tuple(map(float, page)) for page in sizes])

    actual_fonts = []
    for row in run("pdffonts", candidate).splitlines()[2:]:
        fields = row.split()
        if len(fields) < 8:
            raise ValueError(f"unrecognized pdffonts row: {row}")
        name = re.sub(r"^[A-Z]{6}\+", "", fields[0])
        equal(f"{name} embedded/subset/Unicode", ["yes"] * 3, fields[-5:-2])
        actual_fonts.append(name)
    equal("PDF fonts", sorted(fonts), sorted(actual_fonts))

    for page in range(1, len(pages) + 1):
        select = ("-f", str(page), "-l", str(page))
        before = run("pdftotext", *select, "-layout", "-enc", "UTF-8", reference, "-")
        after = run("pdftotext", *select, "-layout", "-enc", "UTF-8", candidate, "-")
        equal(f"PDF page {page} text", before, after)
        # Render both with this host's Poppler so its version and antialiasing are shared.
        for pdf, prefix in [(reference, "reference"), (candidate, "candidate")]:
            run("pdftoppm", *select, "-r", "72", "-singlefile", pdf, candidate.parent / prefix)
        try:
            compare_pixels(candidate.parent / "reference.ppm", candidate.parent / "candidate.ppm")
        except ValueError as error:
            raise ValueError(f"PDF page {page}: {error}") from error


def check_export(cli, name, expected, verified, directory, strict):
    reference = BASE / name
    candidate = directory / name
    candidate.parent.mkdir(exist_ok=True)
    fixture = reference.parent.name
    source = reference.parent / f"{fixture}.vsdx"
    if name == "office-network/page-1.png":
        run(cli, "convert", source, candidate)
    elif candidate.suffix == ".pdf":
        run(cli, "render", source, candidate)
    else:
        page = candidate.stem.removeprefix("page-")
        run(cli, "render", source, "--page", page, candidate)

    if candidate.suffix == ".png":
        compare_pixels(reference, candidate)
    elif candidate.suffix == ".svg":
        compare_svg(ET.parse(reference).getroot(), ET.parse(candidate).getroot())
    elif candidate.suffix == ".pdf":
        check_pdf(reference, candidate, expected["pages_pt"],
                  verified[fixture]["pdf_embedded_fonts_with_unicode_mapping"])
    else:
        raise ValueError(f"unsupported export: {name}")
    if strict:
        equal("file SHA-256", sha256(reference), sha256(candidate))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cli", default="misterdiagram", help="candidate binary (default: PATH)")
    parser.add_argument("--strict-bytes", action="store_true", help="also require identical files")
    args = parser.parse_args()
    baseline = json.loads((BASE / "baseline.json").read_text())
    # Detect stale or accidentally edited references before invoking the candidate.
    for name, digest in baseline["sha256"].items():
        equal(f"{name} reference SHA-256", digest, sha256(BASE / name))
    references = {path.relative_to(BASE).as_posix() for pattern in
                  ("*/page-*.svg", "*/page-*.png", "*/document.pdf") for path in BASE.glob(pattern)}
    if not references or references != baseline["exports"].keys():
        raise ValueError("export inventory differs from the checked-in primary reference files")
    failures = 0
    with tempfile.TemporaryDirectory(prefix="misterdiagram-regression-") as tmp:
        for name, expected in baseline["exports"].items():
            try:
                check_export(args.cli, name, expected, baseline["verified"], Path(tmp),
                             args.strict_bytes)
                print(f"PASS {name}")
            except (OSError, ValueError, ET.ParseError) as error:
                print(f"FAIL {name}: {error}", file=sys.stderr)
                failures += 1
    print(f"{len(baseline['exports'])} exports checked, {failures} failed")
    return int(failures != 0)


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, ValueError) as error:
        sys.exit(str(error))
