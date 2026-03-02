# TODO

- [x] Make sure all 4 steps pass at least once
- [x] Retry CLI tool for note creation
- [x] Add more typed edge to note writer examples + better topics!
- [x] Remove super verbose test output
- [x] Web cache per session
- [x] Review prompt building system in Lua, messages types, ...
- [x] Validate Lua migration by running 4 steps e2e
- [x] Change trigger system: crontab + dispatch only -> deep research triggers
      reflection
- [x] Review reflection building: fork-reflection uses
      ctx:list_messages(args.session_id)
- [x] Recursive calling security
- [x] Remove build_ghost_agents (agents discovered through skills, not system prompt)
- [x] Upgrade deps (cron 0.15, mlua 0.11, rand 0.10, resvg 0.47, reqwest 0.13, toml 1.0)
- [x] Cleanup surrealdb references
- [x] Review CLAUDE skills (symlinks to agent skills for some?)
- [x] Add lua formatter + tooling (StyLua + LuaLS type stubs)
- [ ] Add compaction to agents (pre-emptive or post-error + special compaction prompt)
- [ ] Experimentations
  - [ ] Try deep research `respond` tool: report, sources, negative_information
  - [ ] Re-make agent reflection a fresh session instead of a fork
