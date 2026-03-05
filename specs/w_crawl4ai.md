Goal: HEAVILY review Crawl4ai usage and web fetch

Issues:

- 40s+ per page atm -> review what could be causing this
- Hardcoded strict fetch rules:
  - We should have some deterministic rules
  - We should likely also expose some options to the GHOS

Ideas:

- Give more options to the agent, don't try to do it all ourselves
- In particular, give options regarding crawl4ai
