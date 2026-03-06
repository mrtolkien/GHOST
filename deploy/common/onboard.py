# /// script
# requires-python = ">=3.11"
# dependencies = [
#     "questionary>=2.1.0",
# ]
# ///
"""GHOST interactive onboarding wizard."""

from pathlib import Path
import sys

import questionary

GHOST_CONFIG_DIR = Path.home() / ".config" / "ghost"

PROVIDERS = {
    "OpenRouter": {
        "env_key": "OPENROUTER_API_KEY",
        "config_name": "openrouter",
    },
    "Kimi": {
        "env_key": "KIMI_API_KEY",
        "config_name": "kimi",
    },
    "ChatGPT subscription (OAuth — free, uses your ChatGPT account)": {
        "env_key": None,
        "config_name": "openai_oauth",
    },
}


def select_provider() -> dict:
    provider_name = questionary.select(
        "Select your LLM provider:",
        choices=list(PROVIDERS.keys()),
    ).ask()
    if provider_name is None:
        sys.exit(1)
    return {**PROVIDERS[provider_name], "display_name": provider_name}


def get_api_key(provider: dict) -> str | None:
    if provider["env_key"] is None:
        questionary.print(
            "  ChatGPT OAuth uses your ChatGPT account — no API key needed.",
            style="italic",
        )
        questionary.print(
            "  After install, run `ghost auth codex` to complete the OAuth login.",
            style="italic",
        )
        return None

    api_key = questionary.text(
        f"Enter your {provider['display_name']} API key:",
        validate=lambda v: len(v.strip()) > 0 or "API key cannot be empty",
    ).ask()
    if api_key is None:
        sys.exit(1)
    return api_key.strip()


def get_model_config() -> tuple[str, int]:
    model = questionary.text(
        "Enter the model name (e.g. anthropic/claude-sonnet-4):",
        validate=lambda v: len(v.strip()) > 0 or "Model name cannot be empty",
    ).ask()
    if model is None:
        sys.exit(1)

    ctx_str = questionary.text(
        "Enter the context window size:",
        default="200000",
        validate=lambda v: v.strip().isdigit() or "Must be a number",
    ).ask()
    if ctx_str is None:
        sys.exit(1)

    return model.strip(), int(ctx_str.strip())


def get_discord_config() -> tuple[str, str]:
    token = questionary.text(
        "Enter your Discord bot token:",
        validate=lambda v: len(v.strip()) > 0 or "Token cannot be empty",
    ).ask()
    if token is None:
        sys.exit(1)

    user_id = questionary.text(
        "Enter your Discord user ID:",
        validate=lambda v: v.strip().isdigit() or "User ID must be numeric",
    ).ask()
    if user_id is None:
        sys.exit(1)

    return token.strip(), user_id.strip()


def read_env(path: Path) -> dict[str, str]:
    """Parse an existing .env file into a dict, preserving order."""
    env = {}
    if path.exists():
        for line in path.read_text().splitlines():
            line = line.strip()
            if not line or line.startswith("#"):
                continue
            if "=" in line:
                key, _, value = line.partition("=")
                env[key.strip()] = value.strip()
    return env


def write_env(
    api_key: str | None, env_key: str | None, discord_token: str
) -> None:
    GHOST_CONFIG_DIR.mkdir(parents=True, exist_ok=True)
    env_path = GHOST_CONFIG_DIR / ".env"

    existing = read_env(env_path)
    if existing:
        questionary.print(
            "  Existing .env found — updating managed keys only.",
            style="fg:yellow",
        )

    if api_key and env_key:
        existing[env_key] = api_key
    existing["DISCORD_TOKEN"] = discord_token

    lines = [f"{k}={v}" for k, v in existing.items()]
    env_path.write_text("\n".join(lines) + "\n")
    questionary.print(f"  Wrote {env_path}", style="bold")


def read_toml_lines(path: Path) -> list[str]:
    """Read existing config.toml lines, or return empty list."""
    if path.exists():
        return path.read_text().splitlines()
    return []


def update_toml_section(
    lines: list[str], section: str, values: dict[str, str]
) -> list[str]:
    """Update or append a TOML section with the given key=value pairs.

    Handles top-level sections like [discord] and dotted sections like
    [models.primary]. Preserves all other content.
    """
    section_header = f"[{section}]"
    result = []
    in_section = False
    keys_written: set[str] = set()
    section_found = False

    for line in lines:
        stripped = line.strip()

        # Detect section boundaries
        if stripped.startswith("["):
            if in_section:
                # Write any remaining keys before leaving the section
                for k, v in values.items():
                    if k not in keys_written:
                        result.append(f"{k} = {v}")
                        keys_written.add(k)
                in_section = False

            if stripped == section_header:
                in_section = True
                section_found = True
                result.append(line)
                continue

        if in_section:
            # Check if this line sets a key we want to update
            if "=" in stripped and not stripped.startswith("#"):
                key = stripped.split("=", 1)[0].strip()
                if key in values:
                    result.append(f"{key} = {values[key]}")
                    keys_written.add(key)
                    continue

        result.append(line)

    # If we were still in the section at EOF, flush remaining keys
    if in_section:
        for k, v in values.items():
            if k not in keys_written:
                result.append(f"{k} = {v}")

    # If section didn't exist at all, append it
    if not section_found:
        if result and result[-1].strip():
            result.append("")
        result.append(section_header)
        for k, v in values.items():
            result.append(f"{k} = {v}")

    return result


def toml_val(v: str | int) -> str:
    """Format a value for TOML output."""
    if isinstance(v, int):
        return str(v)
    return f'"{v}"'


def write_config(
    provider_name: str,
    model: str,
    context_window: int,
    discord_user_id: str,
    embeddings_url: str,
) -> None:
    GHOST_CONFIG_DIR.mkdir(parents=True, exist_ok=True)
    config_path = GHOST_CONFIG_DIR / "config.toml"

    lines = read_toml_lines(config_path)
    if lines:
        questionary.print(
            "  Existing config.toml found — updating managed sections only.",
            style="fg:yellow",
        )

    # Update each managed section
    lines = update_toml_section(
        lines, "discord", {"allowed_user_id": toml_val(discord_user_id)}
    )
    lines = update_toml_section(
        lines,
        "models.primary",
        {
            "provider": toml_val(provider_name),
            "model": toml_val(model),
            "context_window": toml_val(context_window),
        },
    )
    lines = update_toml_section(
        lines,
        "embeddings",
        {
            "url": toml_val(embeddings_url),
            "model": toml_val("qwen3-embedding:8b"),
        },
    )
    lines = update_toml_section(
        lines,
        "web",
        {
            "crawl4ai_url": toml_val("http://crawl4ai:11235"),
            "docling_url": toml_val("http://host.docker.internal:5001"),
        },
    )
    lines = update_toml_section(
        lines,
        "web.search",
        {
            "provider": toml_val("searxng"),
            "url": toml_val("http://searxng:8080"),
        },
    )

    config_path.write_text("\n".join(lines) + "\n")
    questionary.print(f"  Wrote {config_path}", style="bold")


def main() -> None:
    questionary.print("\n  Welcome to GHOST setup!\n", style="bold")

    provider = select_provider()
    api_key = get_api_key(provider)
    model, context_window = get_model_config()
    discord_token, discord_user_id = get_discord_config()

    questionary.print("\n  Writing configuration...\n", style="bold")

    write_env(api_key, provider["env_key"], discord_token)

    embeddings_url = "http://host.docker.internal:11434"

    write_config(
        provider["config_name"],
        model,
        context_window,
        discord_user_id,
        embeddings_url,
    )

    questionary.print("\n  GHOST configuration complete!\n", style="bold fg:green")

    if provider["env_key"] is None:
        questionary.print(
            "\n  Remember: run `ghost auth codex` to complete OpenAI OAuth login.\n",
            style="bold fg:yellow",
        )


if __name__ == "__main__":
    main()
