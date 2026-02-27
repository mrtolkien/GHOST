# /// script
# requires-python = ">=3.11"
# dependencies = ["questionary>=2.0.1"]
# ///

"""Manual refresh runner for step-based e2e fixtures.

Usage:
    uv run scripts/e2e/refresh.py
    uv run scripts/e2e/refresh.py --models primary,openai
"""

from __future__ import annotations

import argparse
import os
import subprocess
import tomllib
from pathlib import Path

import questionary


SCENARIO_TESTS = [
    "printer_3d_step_01_spawn_agent",
    "printer_3d_step_02_run_agent_completion",
    "printer_3d_step_03_reflect_agent",
    "printer_3d_step_04_finalize_chat_and_reflect",
]


def discover_model_aliases() -> list[str]:
    config_path = Path.home() / ".config" / "ghost" / "config.toml"
    if not config_path.exists():
        return ["primary"]

    try:
        data = tomllib.loads(config_path.read_text())
        models = data.get("models", {})
        aliases = [k for k, v in models.items() if isinstance(v, dict)]
        if aliases:
            aliases.sort()
            default = data.get("models", {}).get("default")
            if isinstance(default, str) and default in aliases:
                aliases.remove(default)
                aliases.insert(0, default)
            return aliases
    except Exception:
        pass

    return ["primary"]


def run_one(model: str, test_name: str) -> None:
    env = os.environ.copy()
    env["GHOST_E2E_MODEL"] = model

    cmd = [
        "cargo",
        "test",
        "--features",
        "e2e-tests",
        "--test",
        "e2e_steps",
        test_name,
        "--",
        "--nocapture",
        "--test-threads=1",
    ]
    print(f"\n=== model={model} test={test_name} ===")
    subprocess.run(cmd, env=env, check=True)


def interactive_select_models() -> list[str]:
    aliases = discover_model_aliases()
    selected = questionary.checkbox(
        "Select model aliases for fixture refresh",
        choices=aliases,
        validate=lambda a: True if a else "Pick at least one model",
    ).ask()
    if not selected:
        return []
    return list(selected)


def interactive_select_steps() -> list[str]:
    selected = questionary.checkbox(
        "Select e2e steps",
        choices=SCENARIO_TESTS,
        default=SCENARIO_TESTS,
        validate=lambda a: True if a else "Pick at least one step",
    ).ask()
    if not selected:
        return []
    return list(selected)


def run(models: list[str], tests: list[str]) -> None:
    for model in models:
        for test_name in tests:
            run_one(model, test_name)


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Refresh e2e fixtures sequentially")
    parser.add_argument(
        "--models",
        default="",
        help="Comma-separated model aliases. If omitted, interactive picker is used.",
    )
    parser.add_argument(
        "--steps",
        default="",
        help="Comma-separated step test names. If omitted, interactive picker is used.",
    )
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> None:
    args = parse_args(argv)

    models = [m.strip() for m in args.models.split(",") if m.strip()]
    if not models:
        models = interactive_select_models()
    if not models:
        raise SystemExit("No models selected")

    tests = [s.strip() for s in args.steps.split(",") if s.strip()]
    if not tests:
        tests = interactive_select_steps()
    if not tests:
        raise SystemExit("No steps selected")

    run(models, tests)


if __name__ == "__main__":
    main()
