- [ ] Still getting "empty endturn after recovery nudge"
- [ ] Heartbeat is weird: default heartbeat should be empty (no run), but I also can see
      that heartbeat runs don't properly run in the chat session like they should. We
      should DISABLE HEARTBEAT for the moment and create a backlog task for the
      re-activation, properly reviewing the specs.
- [ ] read_file should not use limit: 2000 by default, and those secondary attributes
      should not be in the discord log
- [x] run_after_heartbeat shows as <ongoing> for a long time
- [x] Better logfire traces: 1 tool call = 1 span, 1 response = 1 span, proper model
      response with gen_ai fields, wtf are `llm` spans, add tool responses as log/spans,
      ...
- [x] Better logging: hard to see request / responses atm
- [x] Setup access to logfire logs for Claude to look at spans and logs easily
- [x] Error: database query failed for table 'session' operation 'get': ...
- [x] Setup crawl4ai on my homelab
