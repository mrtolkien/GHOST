- [x] Run GHOST with docker on my homelab
- [x] Review superpowers vendoring: put in /skills/superpowers + also add examples and
      scripts!!!
- [x] Review skills for chat vs coding
- [x] Run Ark Nova e2e after codex is available again (17h)
- [x] Review lua agents for coding -> some of the skills should use agents
- [x] Re-running the deploy scripts nuked the memory
- [x] Review import fails + how do timed out commands behave. When the GHOST tried to do
      a sync import, it timed out. Then it tried a background one, which failed because
      of a UNIQUE CONSTRAINT error.
- [x] Basic generic agent?
- [x] Review agents lua files: should likely be close to the _skills_ that actually
      makes them discoverable

---

- [x] Remove todo for chat session? Overkill, often used for no good reason
- [ ] ?session description for coding agent? -> would help with resume

---

- [x] No statusline for messages that respond to an external trigger
- [x] Add image generation for visual responses -> Nano banana?
- [x] Currently, the GHOST reads the knowledge navigator skill extremely often. This
      highlights two issues:
  - The skill's definition is too broad, it should only get read when the GHOST wants to
    know about its internals
  - The GHOST re-reads it almost every turn: I thought skills would get read maybe once
    or twice per session _max_. Is this normal behaviour?
- [ ] Coupled with that, we should let the GHOST directly query its sqlite database
      somehow. There should be the full schema somewhere (likely as an attachement to
      the knowledge navigator skill) + sqlite3 in the flake (or stg else?)

- [x] In tools use in discord, don't show default values (like background=False for
      shell commands)

- [ ] We should have a _true_ e2e test which actually starts the daemon and interacts
      with it. The only cheating we would do is the provider/Discord layer: we would
      need to be able to directly insert messages to the chat.
  - This would benefit from using an LLM to generate user messages :D
