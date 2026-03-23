- Discord interactions are randomly failing, but I don't understand why/how
- forbid file edit without a read like Claude?
- .cache sometimes does not end up empty

---

Document somewhere (in docs, in cli, anything) what is the current quality status of all
providers integration

For example OpenRouter is 10/10 as it just works. But kimi code, codex, and claude code
are super flaky so they'd likely be 5/10, 4/10, and 1/10.

I daily drive codex atm but will switch to claude or openrouter.

---

- [x] Repair orphaned tool calls _on each response_
- [x] Don't send histories with tool calls that don't have a response -> at least give
      an error if that's the case
- [x] Need some feedback (container w/ color) on reboot
- [x] Note reconciliation on boot is very slow (3s for only 100 notes/references) ->
      need parallelization or running in the background
