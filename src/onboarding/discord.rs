use std::time::Duration;

use super::OnboardingError;

const SETUP_GUIDE: &str = "Your GHOST communicates with you through a Discord bot.
You'll need to create one in the Discord Developer Portal.

1. Go to https://discord.com/developers/applications
2. Click \"New Application\" → name it (e.g. \"GHOST\")
3. Go to \"Bot\" tab:
   → Click \"Reset Token\" → copy the token
   → Enable \"Message Content Intent\" under Privileged Gateway Intents
4. Go to \"OAuth2\" → \"URL Generator\":
   → Check \"bot\" scope
   → Check permissions: Send Messages, Read Message History,
     Attach Files, Use Slash Commands, Embed Links
5. Copy the generated URL → open it → invite the bot to your server";

/// Validate that a Discord user ID is numeric and 17–19 digits.
pub fn validate_user_id(id: &str) -> Result<(), OnboardingError> {
    let len = id.len();
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_digit()) || !(17..=19).contains(&len) {
        return Err(OnboardingError::InvalidInput(format!(
            "Discord user ID must be 17–19 digits, got: {id:?}"
        )));
    }
    Ok(())
}

/// Validate a Discord bot token by calling the Discord API.
///
/// Makes a real HTTP request to confirm the token is accepted.
pub async fn validate_bot_token(token: &str) -> Result<(), OnboardingError> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()?;

    let resp = client
        .get("https://discord.com/api/v10/users/@me")
        .header("Authorization", format!("Bot {token}"))
        .send()
        .await
        .map_err(|e| OnboardingError::DiscordValidation(format!("request failed: {e}")))?;

    if resp.status().is_success() {
        return Ok(());
    }

    let status = resp.status();
    let body = resp
        .text()
        .await
        .unwrap_or_else(|_| "<unreadable body>".to_string());

    Err(OnboardingError::DiscordValidation(format!(
        "HTTP {status}: {body}"
    )))
}

/// Run the Discord setup phase of the onboarding wizard.
///
/// Shows a setup guide, then prompts for (or accepts via flags) a bot token
/// and OPERATOR user ID. Both are validated before returning.
pub async fn prompt_discord(
    token_flag: Option<&str>,
    user_flag: Option<&str>,
) -> Result<(String, String), OnboardingError> {
    cliclack::note("Discord Bot Setup", SETUP_GUIDE)?;

    let token = match token_flag {
        Some(t) => t.to_string(),
        None => cliclack::password("Paste your bot token").interact()?,
    };
    validate_bot_token(&token).await?;

    let user_id: String = match user_flag {
        Some(u) => u.to_string(),
        None => cliclack::input("Your Discord user ID")
            .placeholder(
                "Enable Developer Mode in Settings → Advanced, \
                 then right-click your name → Copy User ID",
            )
            .interact()?,
    };
    validate_user_id(&user_id)?;

    Ok((token, user_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_discord_user_id() {
        assert!(validate_user_id("123456789012345678").is_ok());
        assert!(validate_user_id("12345678901234567").is_ok()); // 17 digits
        assert!(validate_user_id("1234567890123456789").is_ok()); // 19 digits
    }

    #[test]
    fn invalid_discord_user_id() {
        assert!(validate_user_id("abc").is_err());
        assert!(validate_user_id("123").is_err()); // too short
        assert!(validate_user_id("").is_err());
        assert!(validate_user_id("12345678901234567a").is_err()); // has letter
    }
}
