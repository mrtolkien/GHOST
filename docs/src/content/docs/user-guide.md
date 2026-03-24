---
title: User Guide
description: How to talk to your GHOST and get the most out of it.
---

Your GHOST is a personal AI assistant that lives on your computer or homelab. It can
browse the web, run programs, remember things, and learn new workflows. This page covers
what it can do and how to get the best results.

## What your GHOST can do

Your GHOST is not a chatbot trapped in a text box. It has access to:

- **A full terminal** — it can run commands, install software, manage files, and execute
  scripts on your machine.
- **A web browser** — it can search the web, read pages, and pull in information from
  anywhere online.
- **A knowledge base** — it stores notes, references, and diary entries so it remembers
  what matters to you across conversations.
- **Background agents** — it can delegate slow, thorough work to background processes
  while staying responsive to you.

You don't need to explain your entire situation every time. Your GHOST remembers core
information from past conversations and web research.

## Skills: how your GHOST learns

Skills are the main way your GHOST gets better over time. A skill is a plain text file
that teaches your GHOST how to handle a specific type of task — like a recipe it can
follow.

Your GHOST ships with a set of default skills, but the real power is that **you can ask
it to create new ones**. If you find yourself repeating the same kind of request, just
say:

> "Create a skill for how I like my weekly summaries done"

and your GHOST will write a reusable skill it can follow every time.

### Tips for good skills

When asking your GHOST to create a skill:

- **Keep it generic.** A skill for "research a topic and write a summary" is more
  reusable than one for "research Japanese train passes." Tell your GHOST to handle
  specifics through parameters rather than hard-coding them.
- **Iterate.** If the result isn't quite right, tell your GHOST what to fix. It will
  update the skill.

Skills follow the [agentskills.io](https://agentskills.io) format — an open standard for
AI agent skills. You don't need to know the format yourself; your GHOST handles it.

## Key capabilities

### Deep research

When you ask a question that needs real research — comparisons, buying decisions,
multi-factor evaluations — your GHOST won't just give you a quick answer. It will search
multiple sources, read full pages, cross-reference findings, and cite what it found.

For complex questions, your GHOST may delegate the research to a **background agent**.
This means it hands the work off to a separate process that takes its time — searching
broadly, reading carefully, and producing a thorough, accurate response. You'll get a
notification when the research is done. This is slower than a quick reply, but the
results are significantly better for anything that requires depth.

> "What's the best noise-cancelling headphone under $300 right now?"
>
> "Compare the top 3 project management tools for a 5-person team"

### Projects

Your GHOST can manage long-running work that spans multiple conversations. Just tell it
to start a project:

> "Start a project for planning my apartment move in April"

It will create a persistent project with tasks, track progress across sessions, and pick
up where you left off. Projects are great for anything that takes more than one sitting
— trip planning, home renovation research, learning a new subject, or organizing an
event.

### Coding

The GHOST has a dedicated coding agent persona called the Puppet Master. It behaves and
works like a standard coding agent instead of behaving like an assistant. You can spawn
it by asking things like:

> "Clone my keyboard layout repo and swap the inner thumb and pink key"

It will activate, show a clear feedback in the chat, and start working on it.

To leave this mode you can use the `/kill` command or ask the Puppet Master directly.

### Scripting

Your GHOST can write and run scripts to answer questions that need computation — data
processing, file conversions, API calls, calculations, or generating charts. You don't
need to know how to code. Just describe what you need:

> "Parse this CSV and show me the top 10 entries by revenue"
>
> "Download all the images from this folder and resize them to 800px wide"

Your GHOST creates the script, runs it, and gives you the result. Scripts are saved in
your workspace so they can be reused or tweaked later.

### Image generation

Your GHOST can generate images — infographics, visualizations, diagrams, or creative
artwork.

> "Create an infographic comparing the nutritional value of these 5 foods"
>
> "Generate a diagram of my home network setup"

:::note

Image generation requires a Google Gemini API key. Set the `GEMINI_API_KEY` environment
variable in your configuration.

:::

### Reference import

Your GHOST can import entire git repositories or websites into its knowledge base,
making them searchable in natural language. This is useful when you want to ask
questions about documentation, codebases, or any large body of content without manually
reading through it.

> "Import the Astro documentation site so I can ask questions about it"
>
> "Import this GitHub repo and explain how the authentication works"

Once imported, the content is indexed and searchable — you can ask your GHOST questions
about it as naturally as you'd ask a colleague who read the whole thing.

### Self management

Your GHOST has access to its own `ghost` command, letting it update, reboot, or analyze
itself. It also ships with this whole documentation bundled.

If there is anything you do not understand or if you need help on how to use it, just
ask:

> "How do I configure OpenAI OAuth for you?"
>
> "Update yourself and your services"

Just ask, and _normally_ the GHOST should manage to update itself!

## General tips

- **Be direct.** You don't need to be polite or formal. "Find me a good recipe for
  sourdough bread" works great.
- **Give context when it matters.** Your GHOST remembers a lot, but if you're starting
  something new, a sentence of context goes a long way.
- **Ask it to remember things.** "Remember that I'm allergic to shellfish" — and it
  will, across all future conversations.
- **Ask it to create skills** when you notice a pattern. That's the fastest way to make
  your GHOST better at the things you care about.
