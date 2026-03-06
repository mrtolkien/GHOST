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
        "models": [
            ("anthropic/claude-sonnet-4", 200_000),
            ("anthropic/claude-haiku-4", 200_000),
            ("google/gemini-2.5-pro-preview", 1_000_000),
            ("google/gemini-2.5-flash-preview", 1_000_000),
            ("deepseek/deepseek-r1", 64_000),
        ],
    },
    "Kimi": {
        "env_key": "KIMI_API_KEY",
        "config_name": "kimi",
        "models": [
            ("kimi-k2", 128_000),
        ],
    },
    "OpenAI (OAuth — free, uses ChatGPT account)": {
        "env_key": None,
        "config_name": "openai_oauth",
        "models": [
            ("o4-mini", 200_000),
            ("gpt-4.1", 1_000_000),
        ],
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
            "  OpenAI OAuth uses your ChatGPT account — no API key needed.",
            style="italic",
        )
        questionary.print(
            "  Run `ghost auth openai` after install to complete OAuth login.",
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


def select_model(provider: dict) -> tuple[str, int]:
    choices = [f"{name} ({ctx // 1000}k ctx)" for name, ctx in provider["models"]]
    selection = questionary.select(
        "Select your default model:", choices=choices
    ).ask()
    if selection is None:
        sys.exit(1)
    idx = choices.index(selection)
    return provider["models"][idx]


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


def write_env(api_key: str | None, env_key: str | None, discord_token: str) -> None:
    GHOST_CONFIG_DIR.mkdir(parents=True, exist_ok=True)
    env_path = GHOST_CONFIG_DIR / ".env"

    lines = []
    if api_key and env_key:
        lines.append(f"{env_key}={api_key}")
    lines.append(f"DISCORD_TOKEN={discord_token}")

    env_path.write_text("\n".join(lines) + "\n")
    questionary.print(f"  Wrote {env_path}", style="bold")


def write_config(
    provider_name: str,
    model: str,
    context_window: int,
    discord_user_id: str,
    embeddings_url: str,
) -> None:
    GHOST_CONFIG_DIR.mkdir(parents=True, exist_ok=True)
    config_path = GHOST_CONFIG_DIR / "config.toml"

    config = f"""\
[discord]
allowed_user_id = "{discord_user_id}"

[models.primary]
provider = "{provider_name}"
model = "{model}"
context_window = {context_window}

[embeddings]
url = "{embeddings_url}"
model = "qwen3-embedding:8b"

[web]
crawl4ai_url = "http://crawl4ai:11235"
docling_url = "http://host.docker.internal:5001"

[web.search]
provider = "searxng"
url = "http://searxng:8080"
"""
    config_path.write_text(config)
    questionary.print(f"  Wrote {config_path}", style="bold")


def main() -> None:
    questionary.print("\n  Welcome to GHOST setup!\n", style="bold")

    provider = select_provider()
    api_key = get_api_key(provider)
    model, context_window = select_model(provider)
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


if __name__ == "__main__":
    main()
