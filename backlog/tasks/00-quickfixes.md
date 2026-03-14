- Discord interactions are randomly failing, but I don't understand why/how
- .cache sometimes does not end up empty
- Like claude, forbid file edit without a read?

---

- [x] Don't send histories with tool calls that don't have a response -> at least give
      an error if that's the case
- [x] Need some feedback (container w/ color) on reboot
- [x] Note reconciliation on boot is very slow (3s for only 100 notes/references) ->
      need parallelization or running in the background
