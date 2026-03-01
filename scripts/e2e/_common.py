from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path
import json


REPO_ROOT = Path(__file__).resolve().parents[2]
FIXTURES_ROOT = REPO_ROOT / "tests" / "fixtures" / "e2e"
E2E_OUTPUT_ROOT = REPO_ROOT / "e2e-output"


@dataclass(frozen=True)
class StepDir:
    path: Path
    scenario: str
    model: str
    step: str


def list_step_dirs() -> list[StepDir]:
    if not FIXTURES_ROOT.exists():
        return []

    rows: list[StepDir] = []
    for scenario_dir in FIXTURES_ROOT.iterdir():
        if not scenario_dir.is_dir():
            continue
        for model_dir in scenario_dir.iterdir():
            if not model_dir.is_dir():
                continue
            for step_dir in model_dir.iterdir():
                if not step_dir.is_dir():
                    continue
                if not (step_dir / "transcript.json").exists():
                    continue
                rows.append(
                    StepDir(
                        path=step_dir,
                        scenario=scenario_dir.name,
                        model=model_dir.name,
                        step=step_dir.name,
                    )
                )

    rows.sort(key=lambda r: r.path.stat().st_mtime, reverse=True)
    return rows


def step_label(step: StepDir) -> str:
    return f"{step.scenario}/{step.model}/{step.step}"


def read_json(path: Path) -> dict:
    if not path.exists():
        return {}
    return json.loads(path.read_text())


def short(text: str, limit: int = 120) -> str:
    if len(text) <= limit:
        return text
    return text[: limit - 1] + "…"


def list_workspace_dirs() -> list[tuple[str, Path]]:
    """Return (label, path) pairs for all fixture and e2e-output dirs that have a workspace."""
    choices: list[tuple[str, Path]] = []

    # Fixture step dirs (have workspace.tar.zst)
    for sd in list_step_dirs():
        if (sd.path / "workspace.tar.zst").exists():
            choices.append((f"[fixture] {step_label(sd)}", sd.path))

    # e2e-output dirs (have agents/ or skills/ directly)
    if E2E_OUTPUT_ROOT.exists():
        for d in sorted(E2E_OUTPUT_ROOT.iterdir(), reverse=True):
            if not d.is_dir():
                continue
            has_workspace = (
                (d / "agents").exists()
                or (d / "skills").exists()
                or (d / "workspace.tar.zst").exists()
            )
            if has_workspace:
                choices.append((f"[output] {d.name}", d))

    return choices
