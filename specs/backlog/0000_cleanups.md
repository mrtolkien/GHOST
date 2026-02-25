- [x] Better logfire traces: 1 tool call = 1 span, 1 response = 1 span, proper model
      response with gen_ai fields, wtf are `llm` spans, add tool responses as log/spans,
      ...
- [x] Better logging: hard to see request / responses atm
- Setup access to logfire logs for Claude to look at spans and logs easily
- Fix message re-sending on startup: when I restart the GHOST, it always sends me a
  message on Discord. I think it's the default heartbeat that's creating this behaviour.
  Feels a bit weird though, when heartbeat fires normally, this does not happen.
- run_after_heartbeat shows as <ongoing> for a long time
- Still getting "empty endturn after recovery nudge"
- [x] Error: database query failed for table 'session' operation 'get': ...
- [x] Setup crawl4ai on my homelab
