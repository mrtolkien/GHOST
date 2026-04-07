Many skills/CLI tools require tokens and credentials.

We need a reliable, well thought way to pass those to the GHOST while:

- Never leaking them in any logs
- Never having them be sent to providers

Currently I save them in the .env and I have the GHOST pass the value through env
variable substitutions but that's still meh. Could be better.
