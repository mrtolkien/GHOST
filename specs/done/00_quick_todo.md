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
- [x] Add compaction to agents (pre-emptive or post-error + special compaction prompt)
- [x] Message types? User, assistant, system?
- [x] Review docs: flatter? "Features" is vague. Tools don't use the monospace font in
      the section titles.
- [x] Review compaction: should we trigger failures sometimes?
- [x] Compaction reasoning level!
- [x] Review version management in references (version_ref)
- [x] Review topic note: it's wrongly explained in sql = it's not linked to
      references...
- [x] Review skills for import
- [x] Experimentations
  - [x] Try deep research `respond` tool: report, sources, negative_information
  - [x] Re-make agent reflection a fresh session instead of a fork
  - [x] Remove write note tool from deep research
- [x] Review standard reflection agent
- [x] Review compaction token count
- [x] Re-add an agent continuation test
- [x] Test import e2e
- [x] Provider logs have disappeared from logfire (I just see "run tools" spans: span
      fcce30c7019c5d2a)
  - Fixed: renamed openai_oauth span from "request - openai oauth" to "request
    completion" (matching convention). Added tracing::Instrument to agent runner spawns.
- [x] I don't see a span for references import in logfire, at least in the e2e test
  - Fixed: added #[tracing::instrument] to cmd_import, import_git, import_page,
    import_crawl.
- [x] Issues with topic notes and duplicate notes in general: span a0dfcdedce4c6c47 or
      3cbaaa6f6f81cbba or 40192929e911c2ca.
  - [x] The model is note sure how to topic notes + they are poorly written (too much
        content)
  - [x] It tries to create "project" notes... Which makes no sense: we should drop
        archetypes for the moment, they are misleading and will be added back, properly,
        at a later date
    - Fixed: removed archetypes entirely (Archetype enum, DB column, tool param, CLI
      arg, all prompt references). Backlog spec at specs/backlog/archetypes.md.
  - [x] Needs all those changes to be validated through runs of the step 03 and 04 of
        the 3d printer e2e tests
- [x] Web searches are still too biased: they usually include words that come from a
      pre-conceived idea of what the right answer is
  - Fixed: rewrote deep research prompt to avoid training-data brand injection in search
    queries. Added explicit rules about using category terms only.
- [x] Return full deep research report to GHOST: report but also negative info and such
- [x] Try completely removing the todo tool: I'm not sure it's pulling its weight
  - It was: removing it ended up costing many more tokens for worse results
