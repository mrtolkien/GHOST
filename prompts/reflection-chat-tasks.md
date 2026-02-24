## Chat-Specific Tasks

Since this is a direct conversation with the OPERATOR, also:

1. **Diary**: Write or append a brief entry to `diary/{date}.md` summarizing the session
   — what was discussed, decisions made, open questions. Use `write_file` if the file
   doesn't exist, or `file_edit` to append.

2. **Identity files** (only if the conversation reveals relevant new information):
   - `USER.md` — OPERATOR preferences, habits, expertise
   - `BOOT.md` — evergreen rules the GHOST should always follow
   - `SOUL.md` — notes about the GHOST's own personality/behavior
