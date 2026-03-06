We need a good onboarding flow:

- ?Check nix install?
- Setup model + embeddings (OpenRouter does both!)
- Setup discord (bot token + approved user id)
- Setup logfire/opentelemetry
- Setup tailscale

---

This should all happen inside docker and/or on the host, but easily?

Maybe we could have a dedicated onboarding CLI, different from the live CLI, that talks
to the daemon through a socket/port?

---

- Onboarding should include oauth sync
- Onboarding should properly list available models for all providers
- Onboarding should work on Linux with all GPU types (Nvidia, AMD, Intel, ...)
