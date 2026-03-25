#!/usr/bin/env python3
# /// script
# requires-python = ">=3.10"
# dependencies = [
#     "docling>=2.70.0",
#     "onnxruntime",
# ]
# ///
"""
Convert documents to Markdown using docling.

Usage:
    uv run convert.py --path input.pdf --output output.md [--no-ocr] [--page-range 1-10] [--device auto|cpu|cuda|mps]
"""

import argparse
import sys
from pathlib import Path


def parse_page_range(value: str) -> tuple[int, int]:
    """Parse a page range string like '1-10' into a (start, end) tuple."""
    parts = value.split("-")
    if len(parts) != 2:
        raise argparse.ArgumentTypeError(
            f"Page range must be in format 'start-end' (e.g. '1-10'), got: {value!r}"
        )
    try:
        start, end = int(parts[0]), int(parts[1])
    except ValueError:
        raise argparse.ArgumentTypeError(
            f"Page range values must be integers, got: {value!r}"
        )
    if start < 1 or end < start:
        raise argparse.ArgumentTypeError(
            f"Page range must satisfy start >= 1 and end >= start, got: {value!r}"
        )
    return (start, end)


def main():
    parser = argparse.ArgumentParser(
        description="Convert documents to Markdown using docling"
    )
    parser.add_argument(
        "--path",
        required=True,
        help="Path to the input document (e.g. PDF, DOCX)",
    )
    parser.add_argument(
        "--output",
        required=True,
        help="Path to write the output Markdown file",
    )
    parser.add_argument(
        "--no-ocr",
        action="store_true",
        default=False,
        help="Disable OCR (useful for text-layer PDFs, faster)",
    )
    parser.add_argument(
        "--page-range",
        type=parse_page_range,
        default=None,
        metavar="START-END",
        help="Restrict conversion to a page range, e.g. '1-10'",
    )
    parser.add_argument(
        "--device",
        choices=["auto", "cpu", "cuda", "mps"],
        default="auto",
        help="Accelerator device for OCR/layout models (default: auto)",
    )

    args = parser.parse_args()

    input_path = Path(args.path)
    if not input_path.exists():
        print(f"Error: input file not found: {input_path}", file=sys.stderr)
        sys.exit(1)

    output_path = Path(args.output)
    output_path.parent.mkdir(parents=True, exist_ok=True)

    # Imports are deferred to here so that --help and argument errors are instant,
    # without paying the cost of loading docling's heavy dependencies.
    from docling.datamodel.base_models import InputFormat
    from docling.datamodel.pipeline_options import (
        AcceleratorDevice,
        AcceleratorOptions,
        PdfPipelineOptions,
        RapidOcrOptions,
    )
    from docling.document_converter import DocumentConverter, PdfFormatOption

    device_map = {
        "auto": AcceleratorDevice.AUTO,
        "cpu": AcceleratorDevice.CPU,
        "cuda": AcceleratorDevice.CUDA,
        "mps": AcceleratorDevice.MPS,
    }

    accelerator_options = AcceleratorOptions(device=device_map[args.device])

    ocr_options = RapidOcrOptions()
    pipeline_options = PdfPipelineOptions(
        do_ocr=not args.no_ocr,
        ocr_options=ocr_options,
        accelerator_options=accelerator_options,
    )

    # NOTE: --page-range is parsed but not wired up here. docling's Python API for
    # restricting page ranges varies across versions (the field name and accepted type
    # differ between 2.x releases). Wire this up once a stable API surface is confirmed.
    if args.page_range is not None:
        print(
            "Warning: --page-range is not yet supported; the full document will be converted.",
            file=sys.stderr,
        )

    converter = DocumentConverter(
        format_options={
            InputFormat.PDF: PdfFormatOption(pipeline_options=pipeline_options),
        }
    )

    try:
        result = converter.convert(str(input_path))
    except Exception as e:
        print(f"Error converting document: {e}", file=sys.stderr)
        sys.exit(1)

    markdown = result.document.export_to_markdown()

    try:
        output_path.write_text(markdown, encoding="utf-8")
    except OSError as e:
        print(f"Error writing output file: {e}", file=sys.stderr)
        sys.exit(1)

    print(f"Converted: {output_path.resolve()}")


if __name__ == "__main__":
    main()
