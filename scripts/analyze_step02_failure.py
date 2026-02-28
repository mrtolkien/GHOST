#!/usr/bin/env python3
"""
Analyze step_02 e2e test failure by examining the last agent request.

This script inspects the debug request JSON to understand:
- If there was an error in the response
- Input array size and estimated character count
- Whether the agent tried to end its turn or made a function call
- If context/pressure nudge messages were present
"""

# /// script
# requires-python = ">=3.10"
# dependencies = ["jsonlines"]
# ///

import json
import sys
from pathlib import Path


def analyze_request(filepath: str) -> dict:
    """Analyze a single request JSON file."""
    path = Path(filepath)

    if not path.exists():
        return {"error": f"File not found: {filepath}"}

    try:
        with open(path) as f:
            data = json.load(f)
    except json.JSONDecodeError as e:
        return {"error": f"Failed to parse JSON: {e}"}

    result = {
        "filepath": str(path),
        "file_size_kb": round(path.stat().st_size / 1024, 1),
        "iteration": data.get("iteration"),
        "duration_ms": data.get("duration_ms"),
        "model": data.get("model"),
    }

    # Check for top-level error
    if "error" in data:
        result["has_error"] = True
        result["error_details"] = data["error"]
    else:
        result["has_error"] = False

    # Check response for failure events
    response_text = data.get("response", "")
    if isinstance(response_text, str):
        if "response.failed" in response_text:
            result["response_failed"] = True
            # Try to extract error details from response
            if "context_length_exceeded" in response_text:
                result["failure_reason"] = "context_length_exceeded"
                result["failure_message"] = (
                    "Input exceeds context window of model"
                )
            elif "invalid_request_error" in response_text:
                result["failure_reason"] = "invalid_request_error"
        else:
            result["response_failed"] = False

    # Get the request object (nested structure)
    request_obj = data.get("request", {})

    # Analyze input array
    if "input" in request_obj and isinstance(request_obj["input"], list):
        input_array = request_obj["input"]
        result["input_array_length"] = len(input_array)

        # Estimate total character count
        total_chars = 0
        for item in input_array:
            if isinstance(item, dict):
                # Convert to JSON string to estimate size
                item_str = json.dumps(item)
                total_chars += len(item_str)
            elif isinstance(item, str):
                total_chars += len(item)

        result["estimated_total_chars"] = total_chars

        # Check last message type
        if input_array:
            last_item = input_array[-1]
            if isinstance(last_item, dict):
                # Determine message type
                if "content" in last_item:
                    if isinstance(last_item["content"], str):
                        result["last_message_type"] = "text_response"
                        result["last_message_preview"] = (
                            last_item["content"][:100].replace("\n", " ")
                        )
                    elif isinstance(last_item["content"], list):
                        # Mixed content or tool use
                        content_types = []
                        for content_item in last_item["content"]:
                            if isinstance(content_item, dict):
                                if "type" in content_item:
                                    content_types.append(content_item["type"])
                        result["last_message_type"] = "mixed"
                        result["last_message_content_types"] = content_types
                elif "tool_use" in last_item:
                    result["last_message_type"] = "tool_use"
                else:
                    result["last_message_type"] = "unknown"
                    result["last_message_keys"] = list(last_item.keys())

        # Check for context/pressure nudge in system messages
        context_nudges = []
        pressure_nudges = []
        for i, item in enumerate(input_array):
            if isinstance(item, dict):
                role = item.get("role", "")
                content = item.get("content", "")

                if isinstance(content, str):
                    if "context" in content.lower():
                        context_nudges.append(i)
                    if "pressure" in content.lower():
                        pressure_nudges.append(i)

        result["context_nudge_found"] = bool(context_nudges)
        result["context_nudge_indices"] = context_nudges
        result["pressure_nudge_found"] = bool(pressure_nudges)
        result["pressure_nudge_indices"] = pressure_nudges
    else:
        result["input_array_length"] = 0
        result["estimated_total_chars"] = 0
        result["context_nudge_found"] = False
        result["context_nudge_indices"] = []
        result["pressure_nudge_found"] = False
        result["pressure_nudge_indices"] = []

    return result


def main():
    filepath = (
        "/home/tolki/Development/ghost/e2e-output/"
        "2026-02-28T12-51-43_printer_3d_step_02_run_agent_completion/"
        "debug/requests/20260228T125140.138_01RESK90_22.json"
    )

    print("Analyzing step_02 e2e test failure...\n")

    analysis = analyze_request(filepath)

    # Print summary
    print("=" * 70)
    print("STEP_02 FAILURE ANALYSIS")
    print("=" * 70)

    if "error" in analysis:
        print(f"ERROR: {analysis['error']}")
        return 1

    print(f"File: {Path(analysis['filepath']).name}")
    print(f"File size: {analysis['file_size_kb']} KB")
    print(f"Iteration: {analysis.get('iteration', 'N/A')}")
    print(f"Duration: {analysis.get('duration_ms', 'N/A')} ms")
    print(f"Model: {analysis.get('model', 'N/A')}")
    print()

    print("REQUEST ANALYSIS:")
    print("-" * 70)
    if analysis.get("response_failed"):
        print("RESPONSE STATUS: FAILED")
        print(
            f"  Failure reason: {analysis.get('failure_reason', 'unknown')}"
        )
        print(f"  Message: {analysis.get('failure_message', 'N/A')}")
    else:
        print("RESPONSE STATUS: Not explicitly failed")

    print(f"Has error in response: {analysis['has_error']}")
    if analysis.get("error_details"):
        print(f"  Error details: {analysis['error_details']}")
    print()

    print(f"Input array size: {analysis['input_array_length']} items")
    print(f"Estimated total chars: {analysis['estimated_total_chars']:,}")
    print()

    print(f"Last message type: {analysis.get('last_message_type', 'N/A')}")
    if "last_message_preview" in analysis:
        print(f"  Preview: {analysis['last_message_preview']}...")
    if "last_message_content_types" in analysis:
        print(f"  Content types: {analysis['last_message_content_types']}")
    print()

    print("NUDGE DETECTION:")
    print("-" * 70)
    print(f"Context nudge found: {analysis['context_nudge_found']}")
    if analysis["context_nudge_found"]:
        print(f"  At indices: {analysis['context_nudge_indices']}")
    print(f"Pressure nudge found: {analysis['pressure_nudge_found']}")
    if analysis["pressure_nudge_found"]:
        print(f"  At indices: {analysis['pressure_nudge_indices']}")
    print()

    print("=" * 70)

    return 0


if __name__ == "__main__":
    sys.exit(main())
