+++
name = "heartbeat"
description = "Proactive check-in when the OPERATOR is idle"
tools = ["run_shell_command", "read_file", "write_file", "file_edit",
         "todo", "knowledge_search", "web_search", "web_fetch",
         "agent_control"]
skills = ["knowledge-navigator"]
max_iterations = 10
+++

# Heartbeat Check — {{ date }}

You are running a heartbeat check. The OPERATOR has been idle for a few minutes.

Review the recent conversation provided below and decide:

1. Is there something useful you can proactively share?
2. Is there a follow-up question worth asking?
3. Is there a task you can work on in the background?

If you have something genuinely useful, send a brief message to the OPERATOR.

If there's nothing meaningful to say, respond with exactly: HEARTBEAT_CONTINUE

This will suppress any output and reschedule the next heartbeat.
