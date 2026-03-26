- Discord interactions are randomly failing, but I don't understand why/how
- forbid file edit without a read like Claude?
- .cache sometimes does not end up empty
- [ ] References import topics are messy af
- [ ] Review docs bundling: my GHOST (192.168.1.3) re-imported them as a reference. How
      come?

---

- [x] Repair orphaned tool calls _on each response_
- [x] Don't send histories with tool calls that don't have a response -> at least give
      an error if that's the case
- [x] Need some feedback (container w/ color) on reboot
- [x] Note reconciliation on boot is very slow (3s for only 100 notes/references) ->
      need parallelization or running in the background
