#!/usr/bin/env python3
# /// script
# requires-python = ">=3.10"
# dependencies = [
#     "pymupdf",
# ]
# ///
"""Render a single PDF page to a PNG image.

Usage:
    uv run render_page.py --path input.pdf --page 1 --output page.png [--dpi 300]
"""

import argparse
import sys
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(
        description="Render a single PDF page to PNG"
    )
    parser.add_argument("--path", required=True, help="Path to the PDF file")
    parser.add_argument(
        "--page", required=True, type=int, help="Page number (1-based)"
    )
    parser.add_argument("--output", required=True, help="Output PNG path")
    parser.add_argument(
        "--dpi", type=int, default=300, help="Resolution in DPI (default: 300)"
    )

    args = parser.parse_args()

    input_path = Path(args.path)
    if not input_path.exists():
        print(f"Error: file not found: {input_path}", file=sys.stderr)
        sys.exit(1)

    output_path = Path(args.output)
    output_path.parent.mkdir(parents=True, exist_ok=True)

    import pymupdf

    doc = pymupdf.open(str(input_path))
    page_index = args.page - 1
    if page_index < 0 or page_index >= len(doc):
        print(
            f"Error: page {args.page} out of range (1-{len(doc)})",
            file=sys.stderr,
        )
        sys.exit(1)

    page = doc[page_index]
    pix = page.get_pixmap(dpi=args.dpi)
    pix.save(str(output_path))
    print(f"Rendered page {args.page}: {output_path.resolve()}")


if __name__ == "__main__":
    main()
