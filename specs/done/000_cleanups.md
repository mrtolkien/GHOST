- [x] Still getting "empty endturn after recovery nudge"
- [x] read_file: removed limit/offset params, always returns full file
- [x] Heartbeat disabled — see `specs/backlog/heartbeat-reactivation.md`
- [x] run_after_heartbeat shows as <ongoing> for a long time
- [x] Better logfire traces: 1 tool call = 1 span, 1 response = 1 span, proper model
      response with gen_ai fields, wtf are `llm` spans, add tool responses as log/spans,
      ...
- [x] Better logging: hard to see request / responses atm
- [x] Setup access to logfire logs for Claude to look at spans and logs easily
- [x] Error: database query failed for table 'session' operation 'get': ...
- [x] Setup crawl4ai on my homelab
