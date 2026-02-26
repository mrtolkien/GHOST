# Agent e2e test

We are now focusing on a _full_ flow.

## Flow

Act:

- The user asks for recommendations about enclosed 3d printers
- The user question contains all necessary information to respond properly

Assert:

- The deep research agent should be spawned
- It should check the same things as ./tests/deep_research_live.rs
  - Get at least 1 specialist, finds the P2S, ...
- The reflection is high quality and properly works with agents
  - There should be product note for the P2S
  - There should be a source note for all3dp or aurora tech channel

## Authorized changes

There are two things to work on:

- Agent and reflection integration
  - Try running reflection on the agent session after it finishes
  - If the results are not satisfying, try running the reflection from the main chat, it
    will be able to check the web cache itself
- Reflection prompt engineering and nudges
  - Just like the deep research agent, we need to make sure the reflection agent follows
    the instructions _well_ by validating the quality of the notes that were created

## Forbidden changes

We should not:

- Make the reflection prompt specific to either deep research or product research or 3d
  printers
- Create low quality code and hacks by cheating to pass the tests

If we do not succeed with our current setup, maybe we need deeper architectural changes.
