- We currently reconstruct full session context from db + skills and all _every message_
  at the moment
- This is both extremely complex and prone to issues that would make use _miss cache
  hit_: a single character difference ruins THE WHOLE CACHE
- Default setup should therefore be: active sessions live in memory, and we reconstruct
  from scratch ONLY when skills changed or after a reboot
