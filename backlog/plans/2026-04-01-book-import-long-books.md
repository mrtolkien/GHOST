# Book Import: Long Book Support

> **For agentic workers:** REQUIRED SUB-SKILL: Use
> superpowers:subagent-driven-development (recommended) or superpowers:executing-plans
> to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the book-import agent handle books that exceed the LLM context window by
processing chapters progressively instead of dumping everything in one message.

**Architecture:** The `build()` hook in `agent.lua` measures total content size. If it
fits in ~80% of the context window, it uses the current "dump everything" approach. If
too large, it provides a chapter manifest and a progressive-processing prompt that
instructs the agent to read chapters in batches via `file_read`, creating/updating notes
after each batch. A secondary fix adds 6 missing context overflow error patterns
(Anthropic, Gemini, Kimi) and fixes Anthropic's 429 handler to check for context
overflow before misclassifying as rate limited.

**Tech Stack:** Lua (agent definition), Rust (provider error classification)

---

### Task 1: Fix context overflow error detection across all providers

The `is_context_overflow_message()` function is missing patterns from Anthropic, Google
Gemini, and Kimi. Additionally, Anthropic's 429 handler fires before the context
overflow check, so "Extra usage is required for long context requests" gets
misclassified as `RateLimited`.

**Files:**

- Modify: `src/providers/types.rs:253-266`
- Modify: `src/providers/anthropic/mod.rs:196-206`

- [ ] **Step 1: Add missing context overflow patterns for all providers**

In `src/providers/types.rs`, replace `is_context_overflow_message()` with comprehensive
patterns. Each new pattern has a comment explaining which provider emits it:

```rust
pub fn is_context_overflow_message(msg: &str) -> bool {
    let lower = msg.to_lowercase();
    // OpenAI Responses API: "Your input exceeds the context window of this model"
    lower.contains("exceeds the context window")
        // OpenAI Responses API (alt): "the context window of this model"
        || lower.contains("context window of this model")
        // OpenAI Chat Completions / OpenRouter: "This model's maximum context length is N tokens"
        || lower.contains("maximum context length")
        // OpenAI error code (JSON field): "context_length_exceeded"
        || lower.contains("context_length_exceeded")
        // OpenAI error code (in message text): "context length exceeded"
        || lower.contains("context length exceeded")
        // General: "too many tokens"
        || lower.contains("too many tokens")
        // General: "token limit exceeded"
        || lower.contains("token limit exceeded")
        // Anthropic: "prompt is too long: N tokens > M maximum"
        || lower.contains("prompt is too long")
        // General: "input is too long"
        || lower.contains("input is too long")
        // Legacy: "prompt_length exceeded"
        || lower.contains("prompt_length exceeded")
        // OpenAI GPT-5: "Input tokens exceed the configured limit of N"
        || lower.contains("input tokens exceed")
        // Anthropic: "input length and `max_tokens` exceed context limit: N + M > L"
        || lower.contains("exceed context limit")
        // Anthropic 429: "Extra usage is required for long context requests"
        || lower.contains("extra usage is required")
        // Gemini: "The input token count (N) exceeds the maximum number of tokens allowed (M)"
        || lower.contains("exceeds the maximum number of tokens")
        // Gemini Vertex AI: "input token count is N but model only supports up to M"
        || lower.contains("input token count")
        // Kimi: "Input token length too long"
        || lower.contains("token length too long")
        // Kimi: "Your request exceeded model token limit: N"
        || lower.contains("exceeded model token limit")
}
```

- [ ] **Step 2: Check for context overflow in the 429 handler**

In `src/providers/anthropic/mod.rs`, inside the `TOO_MANY_REQUESTS` block, check the
response body for context overflow before falling through to the rate limit path:

```rust
if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
    self.circuit_breaker.record_failure(&request.model);
    // Anthropic returns 429 for long-context billing errors
    // (e.g. "Extra usage is required for long context requests").
    // Classify these as ContextOverflow so the retry path compacts
    // history instead of waiting for a rate-limit reset.
    if ProviderError::is_context_overflow_message(&response_body) {
        return Err(ProviderError::ContextOverflow(format!(
            "HTTP {status}: {response_body}"
        )));
    }
    tracing::warn!(
        provider = "anthropic",
        model = request.model.clone(),
        retry_after_secs = retry_after_secs,
        raw_response = response_body.clone(),
        "anthropic provider rate limited",
    );
    return Err(ProviderError::RateLimited { retry_after_secs });
}
```

- [ ] **Step 3: Run `just ci` and verify it passes**

Run: `just ci` Expected: All checks pass, no clippy warnings.

- [ ] **Step 4: Commit**

```bash
git add src/providers/types.rs src/providers/anthropic/mod.rs
git commit -m "fix: comprehensive context overflow detection across all providers

Add missing patterns for Anthropic ('exceed context limit', 'extra usage
is required'), Gemini ('exceeds the maximum number of tokens', 'input
token count'), and Kimi ('token length too long', 'exceeded model token
limit'). Also fix Anthropic 429 handler to check for context overflow
before classifying as RateLimited — 'Extra usage is required for long
context requests' was silently misclassified."
```

---

### Task 2: Create progressive processing prompt

A new prompt for when the book is too large to fit in context at once. The workflow
shifts from "everything is in the user message" to "read chapters on demand via
file_read, build notes incrementally."

**Files:**

- Create: `assets/agents/book-import/prompt-progressive.md`

- [ ] **Step 1: Write the progressive prompt**

Create `assets/agents/book-import/prompt-progressive.md`:

```markdown
You are creating knowledge notes from an imported book. The book is too large to read at
once — you have a chapter manifest in the user message. Read chapters progressively
using `file_read` and build notes as you go.

## Workflow

### Step 1: Discover existing knowledge

Before reading anything, search for what already exists. Use `knowledge_search` in
parallel:

- Search for the author's name
- Search for 2-3 key concepts from the book title (use general terms)

This tells you what to create vs. update.

### Step 2: Read and process in batches

Read chapters in batches of 2-3 using `file_read`. Focus on content chapters — **skip
files that are clearly indexes, endnotes, appendices, or front matter** (you can tell
from filenames and sizes in the manifest). Prioritize intro, numbered chapters, and
conclusion.

After each batch:

1. Create or update your notes with `note_write` (use `action: update` to add insights
   from later chapters to existing notes)
2. Move on to the next batch

Your notes are your persistent memory — earlier chapters will be compacted from context
as you proceed, but your notes remain in the knowledge store and can be re-read via
`knowledge_search`.

### Step 3: Finalize

After processing all content chapters, do a final `knowledge_search` for the book title
to review your notes. Make any final updates.

## Notes to create

### Source note (the book)

- `archetype: source`, `trust: 7`
- Title: the book's title exactly (e.g., "Animal Farm")
- Write a structured summary (400-800 words): central thesis or narrative arc, how the
  author builds their argument or story, key ideas, what makes this work distinctive
- Include **specific details** — quotes, character names, arguments, examples from the
  text. Vague summaries are worthless.
- Tag: `books/{genre}` (e.g., `books/fiction`, `books/economics`)
- Link to concepts with `[[explores>Concept]]` and author with `[[by>Author Name]]`
- `sources: ["books/{topic-slug}"]` (the reference topic path)
- **Create this after reading the first batch.** Update it as you read more chapters.

### Author note

- `archetype: entity`, `trust: 5`
- Title: the author's full name
- If a note about them already exists (from the search), use `action: update` to add a
  `[[wrote>Book Title]]` link and any new biographical context from this book
- If new: write key biographical context relevant to understanding their work, their
  style, other notable works. Link with `[[wrote>Book Title]]`.
- Tag: `people/authors`

### Concept notes (1-3)

Create notes for the book's major ideas. These must be **general concepts**, not
book-specific sub-topics:

- Good: "Totalitarianism", "Propaganda", "Class Struggle"
- Bad: "Totalitarianism in Animal Farm", "Squealer's Propaganda Techniques"

The concept note should define the concept generally, then note how this book engages
with it. This way the note accumulates links from multiple books over time.

- If the concept already exists (from the search), use `action: update` to add a
  `[[explored_in>Book Title]]` link
- If new: define the concept in 100-300 words, link with `[[explored_in>Book Title]]`
  and `[[by>Author Name]]`
- Fiction: `archetype: entity`, focus on themes — not plot
- Non-fiction: `archetype: analysis`, focus on the argument framework
- `trust: 5`
- Tag: a general topic path (e.g., `politics`, `philosophy/ethics`, `history`)
- **Wait until you've read at least half the chapters** before creating concept notes,
  so you can identify the truly central ideas.

## Genre determines focus

- **Fiction**: extract **themes** — power, freedom, identity, morality. Think literary
  analysis. Characters only matter when they embody a theme.
- **Non-fiction**: extract **logic** — what is the thesis, what evidence supports it,
  what frameworks does the author introduce?

## Linking

Every note must have wiki links. Use typed edges:

| Edge                   | Use for                     |
| ---------------------- | --------------------------- |
| `[[explores>X]]`       | Source note → concept       |
| `[[by>Author]]`        | Source note → author        |
| `[[wrote>Book]]`       | Author note → book          |
| `[[explored_in>Book]]` | Concept note → book         |
| `[[from>Book]]`        | Any note citing this book   |
| `[[compares>Work]]`    | Referencing other works     |
| `[[relates_to>X]]`     | Connecting related concepts |

## After processing all chapters, end your turn.

Do not read back notes you created. Do not update skeleton index notes. Your job is
done.

---

{{note_skill}}
```

- [ ] **Step 2: Commit**

```bash
git add assets/agents/book-import/prompt-progressive.md
git commit -m "feat: add progressive processing prompt for book-import agent

Supports books too large to fit in context at once. Agent reads chapters
in batches via file_read and builds notes incrementally."
```

---

### Task 3: Rewrite agent.lua with size-aware build()

The `build()` function now measures total book content size. Below ~80% of the context
window (~640KB for a 200K-token model), it dumps everything (current behavior). Above
that threshold, it provides a chapter manifest and uses the progressive prompt.

Also raise `max_iterations` to 200 to accommodate progressive processing of long books.

**Files:**

- Modify: `assets/agents/book-import/agent.lua`

- [ ] **Step 1: Write the updated agent.lua**

```lua
local template = require("ghost.template")

--- Bytes threshold for fitting book content in a single user message.
--- ~80% of 200K-token context window at ~4 bytes/token = 640KB.
--- Leaves room for system prompt, tools, and output tokens.
local MAX_SINGLE_SHOT_BYTES = 640000

return {
    name = "book-import",
    description = "Create structured notes from imported book chapters",

    max_iterations = 200,

    tools = {
        "file_read",
        "knowledge_search",
        "note_write",
        "shell",
    },

    skills = { "note-writer" },

    build = function(ctx, args)
        local topic = args.topic or error("book-import requires args.topic")
        local title = args.title or "Unknown"
        local authors = args.authors or "Unknown"

        local note_skill = load_skill("note-writer")

        -- List chapter files with sizes (skip _ prefixed metadata files)
        local ls_result = ctx:call_tool("shell", {
            command = "stat --format='%s %n' references/"
                .. topic
                .. "/*.md 2>/dev/null | grep -v '/_' | sort -t/ -k2 -V",
            timeout_ms = 10000,
        })

        -- Parse file list and compute total size
        local files = {}
        local total_bytes = 0
        for line in ls_result:gmatch("[^\n]+") do
            local size, path = line:match("^(%d+)%s+(.+)$")
            if size and path then
                size = tonumber(size)
                if size >= 200 then -- skip trivial files (covers, part dividers)
                    table.insert(files, { size = size, path = path })
                    total_bytes = total_bytes + size
                end
            end
        end

        local header = "**Title**: "
            .. title
            .. "\n"
            .. "**Author(s)**: "
            .. authors
            .. "\n"
            .. "**Topic (reference path)**: "
            .. topic

        if total_bytes <= MAX_SINGLE_SHOT_BYTES then
            -- Tier 1: fits in context — dump everything in one shot
            local book_result = ctx:call_tool("shell", {
                command = "cat references/" .. topic .. "/*.md",
                timeout_ms = 30000,
            })

            local system_prompt = template.render(read_file("prompt.md"), {
                note_skill = note_skill,
            })

            local user_message = "Create notes for the following book.\n\n"
                .. header
                .. "\n\n## Full Book Text\n\n"
                .. book_result
                .. "\n\n---\n\n"
                .. "The full text is above. Now create the notes."

            return {
                system_prompt = system_prompt,
                messages = { { role = "user", content = user_message } },
            }
        else
            -- Tier 2: too large — progressive chapter-by-chapter processing
            local manifest = ""
            for _, f in ipairs(files) do
                local kb = string.format("%.1f", f.size / 1024)
                manifest = manifest .. "- `" .. f.path .. "` (" .. kb .. " KB)\n"
            end

            local system_prompt = template.render(
                read_file("prompt-progressive.md"),
                { note_skill = note_skill }
            )

            local user_message = "Create notes for the following book.\n\n"
                .. header
                .. "\n\n## Chapter Manifest\n\n"
                .. manifest
                .. "\nTotal: "
                .. string.format("%.0f", total_bytes / 1024)
                .. " KB across "
                .. #files
                .. " files\n\n---\n\n"
                .. "The book is too large to fit in context at once. "
                .. "Read chapters progressively using `file_read` and create notes as you go."

            return {
                system_prompt = system_prompt,
                messages = { { role = "user", content = user_message } },
            }
        end
    end,
}
```

- [ ] **Step 2: Run `just ci` and verify it passes**

Run: `just ci` Expected: All checks pass.

- [ ] **Step 3: Commit**

```bash
git add assets/agents/book-import/agent.lua
git commit -m "feat: book-import agent handles books exceeding context window

build() now measures total content size. Books under ~640KB (80% of 200K
context) use the current single-shot approach. Larger books get a chapter
manifest and progressive processing prompt — agent reads chapters in
batches via file_read and builds notes incrementally.

max_iterations raised to 200 to accommodate long progressive runs."
```

---

### Task 4: Test

Run the existing epub_agent_creates_notes test (requires `live-tests-llms` feature) to
verify the small-book path (Animal Farm, ~135KB) still works correctly.

**Files:**

- Test: `tests/epub_import.rs`

- [ ] **Step 1: Run existing live test**

Run:
`cargo test --features live-tests-llms epub_agent_creates_notes -- --nocapture 2>&1 | tail -30`
Expected: Test passes. Animal Farm (~135KB total) takes the single-shot path and creates
source + author + concept notes.

- [ ] **Step 2: Manual validation of progressive path**

Convert and import Mute Compulsion, then run the agent manually to verify the
progressive path works:

```bash
# Convert
ghost convert epub --path "/home/tolki/Documents/books/Mute Compulsion.epub"

# Import
ghost reference import \
  --path ~/.config/ghost/.staging/mute-compulsion \
  --topic books/mute-compulsion \
  --source-type book

# Run agent (via Discord or CLI chat)
# Ask ghost: "Import the book Mute Compulsion by Søren Mau from books/mute-compulsion"
```

Expected: Agent receives chapter manifest, reads chapters in batches of 2-3, creates
source note, author note, and 1-3 concept notes. No context overflow errors.

- [ ] **Step 3: Commit any test fixes if needed**
