- [ ] Need typing indicator on re-triggers from agents/bg
- [ ] The message sent after bg commands does not appear in chat, but the GHOST think it
      did:
      https://logfire-eu.pydantic.dev/mrtolkien/ghost?q=trace_id%3D%2707dc41d2a1fcc977f52e3819d94bb9d9%27+and+span_id%3D%2732ea28ebc7a1e677%27&spanId=32ea28ebc7a1e677&traceId=07dc41d2a1fcc977f52e3819d94bb9d9&env=-clear-&since=2026-03-28T12%3A08%3A28.660850Z&until=2026-03-28T13%3A08%3A37.346846Z
- Discord interactions are randomly failing, but I don't understand why/how
- forbid file edit without a read like Claude?
- .cache sometimes does not end up empty?
- [ ] References import topics are messy af
- [ ] Claude token usage is wrong
- [ ] There are often pointless `cd /root/GHOST`
- [ ] Agents/cron: GHOST tried ghost agent run, got some syntax wrong and bad feedback,
      UI is unclear on failure, no way to see direct agent message, ...
- [ ] Review all tokio::process::Command to use the nix shell instead
- [ ] Boot takes 2 minutes:
      https://logfire-eu.pydantic.dev/mrtolkien/ghost?q=trace_id%3D%277b13fe106c01ff839650bfe31cef12cb%27+and+span_id%3D%27973c9398cf1d92ca%27&spanId=973c9398cf1d92ca&traceId=7b13fe106c01ff839650bfe31cef12cb&env=-clear-&since=2026-03-28T12%3A06%3A08.958227Z&until=2026-03-28T13%3A06%3A08.958227Z

---

- [x] Review docs bundling: my GHOST (192.168.1.3) re-imported them as a reference. How
      come?
- [x] Repair orphaned tool calls _on each response_
- [x] Don't send histories with tool calls that don't have a response -> at least give
      an error if that's the case
- [x] Need some feedback (container w/ color) on reboot
- [x] Note reconciliation on boot is very slow (3s for only 100 notes/references) ->
      need parallelization or running in the background
