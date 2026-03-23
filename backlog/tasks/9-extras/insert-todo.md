I feel like some skills (like brainstorming in superpowers) should insert a TODO that
only gets passed _after the last message_. This could be done through an agent in lua,
or a tool call?

The idea is that skills teaching a workflow are great... if the model follows the
workflow. When there's a lot of back and forth and messages, some steps can end up being
skipped. For example, the brainstorm -> design plan workflow works, but in Claude Code
it sometimes fails to properly review the plan, despite it being outlined in the skill!

It's messy but to say it otherwise: GHOST and coding agents perform better when taught
clear workflows. That was the case with the deep research agent. That's what superpowers
do for coding.

To guarantee that, we need to have a proper way to guarantee adherence to workflows. In
the deep research agent we do extremly agressive TODO ingestion, but there has to a
better way that also generalizes to superpowers.
