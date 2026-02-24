# Reflection Agent — Experiment Journal

## Test: `reflection_on_agent_transcript`

Isolated test using a pre-captured agent transcript (16 messages, 25 web cache files)
from a successful deep-research run about enclosed 3D printers ~$1000.

**Assertions:**

- Reflection produces a non-empty handoff
- At least one note or reference is created
- A note mentioning "P2S" exists
- A source quality note for "all3dp" or "aurora" exists

---

## Baseline observations

The deep-research agent prompt works well because:

1. **Generous iterations** (50) — model has room to work
2. **Progress nudges** — system injects web_fetch counts via `[Progress]` messages
3. **Explicit "text-only = session ends" warning** — prevents premature reporting
4. **Self-check before reporting** — forces verification loop
5. **Clear step numbering** — model knows exactly what to do and when

The reflection prompt lacks ALL of these. It has a 9-step workflow but:
- No progress tracking
- No nudges about remaining work
- No warning that text-only = failure
- 25 iterations (default) which may not be enough for 25 web cache files + notes
- The model sees the 9-step workflow as advisory, not mandatory

## Ideas to try

1. **Progress nudges for reflection** — inject note_write count + remaining web cache
   files, similar to deep-research's web_fetch nudge
2. **Pass agent context directly** — instead of a filtered transcript, give reflection
   the full agent session (including tool results with page content). This avoids the
   model needing to re-read files it already has content for.
3. **Increase max_iterations** — 25 may not be enough for: discover structure + curate
   25 cache files + create ~8 entity notes + source notes + diary + identity
4. **Trim tool set** — remove tools that distract (web_search, web_fetch for now? or
   keep for augmentation but deprioritize?)
5. **Simplify the workflow** — 9 steps may be too many. The model gets lost. Focus on
   the core: curate cache → create notes → handoff.
6. **Add "text-only = failure" warning** — same pattern as deep-research agent

## Priority order

Start with structural changes (iterations, nudges, context) before prompt tweaks.

---

## Attempt 1: Baseline (pre-changes)

**Prompt state:** Original reflection.md (before this session's changes)

**Result:** Reflection produced empty transcripts in the e2e test. Likely transient
provider issue (empty EndTurn). Not enough data to diagnose reflection quality.

---

## Attempt 2: Added CRITICAL directive

**Hypothesis:** Model treats reflection as a planning exercise, not an execution task.
Adding "You MUST use tools. Text-only = failure. Call note_write at least once."

**Changes:**

- `prompts/reflection.md`: Added CRITICAL blockquote before workflow steps

**Result:** PARTIAL — model started using tools!

- Created 2 notes (1 topic "3D Printing" + 1 other)
- Moved 1 reference from web cache (with malformed filename)
- But stopped after ~3-4 tool calls and wrote a text handoff listing remaining work
- Handoff says: "I did not execute tools in this run" → "I need a corrective pass"
- P2S note: NO
- Source quality note: NO

**Conclusion:** The CRITICAL directive got the model to start using tools, but it still
treats the handoff as an early exit ramp. It plans more than it executes. Needs:
- Progress nudges to keep it going
- More explicit "complete ALL steps before handoff"
- Possibly more iterations

---

## Attempt 3: Prompt restructure + agent findings extraction

**Hypothesis:** Model bails early because (a) the prompt is too long with domain-specific
examples that don't match the actual task, (b) the agent's synthesized report is buried
in `[assistant]` transcript lines instead of being prominent, and (c) `web_search` +
`web_fetch` tools distract from the curation task.

**Changes:**

- `prompts/agents/reflection.md`:
  - Removed `web_search`, `web_fetch` from tool list (reflection synthesizes, doesn't
    re-research)
  - Replaced domain-specific examples (Bambu P2S, All3DP, 3D Printer Decision) with
    generic software examples (Tokio, docs.rs, HTTP Framework Decision)
  - Simplified workflow from 9 steps to 4: discover → create/update notes → curate
    web cache → handoff
  - Improved progress nudges: "You have written {count} notes so far. Is this enough
    to cover all the new information from the agent findings?"
  - Emphasized update-over-create in workflow step 2
- `src/jobs/reflection.rs`:
  - Added `extract_agent_findings()` — pulls last assistant message >= 500 chars as the
    synthesized research report
  - Added `agent_findings` parameter to `build_reflection_user_message()` — presented
    as a dedicated "Agent Findings" section above the transcript
- `tests/common.rs`: Updated `run_reflection` to extract and pass agent findings
- `tests/heartbeat_reflection.rs`: Improved diagnostics — logs note paths + first 200
  chars, reference paths, tier results (T1 hard assert, T2/T3 soft log)

**Result:** T1=PASS, T2=PASS, T3=MISS — massive improvement!

- **9 notes** created (vs 2 in attempt 2):
  - Topic hubs: 3dprinting, enclosed-printers, printers, buying-decisions
  - Entity notes: Prusa CORE One, Bambu P1S AMS, Bambu P2S AMS
  - Topic note: Enclosed 3D Printers Around $1000 (2026)
  - Decision note: Best Enclosed Home Printer Decision (2026-02)
- **T2 all pass**: entity note, source quality note, decision note
- **T3 miss**: 0 references curated — handoff says "not completed yet, pending curation
  pass". The model listed what it would do but didn't execute `reference_manage` calls.
- Completed in ~57 seconds (well within 3-minute timeout)

**What worked:**
- Agent findings extraction: model had the research report front and center
- Simplified 4-step workflow: model followed discover → notes → (attempted cache) → handoff
- Generic examples: no domain contamination from the prompt examples
- Progress nudges: model got feedback on note count

**Remaining gap:**
- Reference curation (T3): model planned it but deferred. Possible causes:
  - Still too many steps? Notes + references in one pass may be too ambitious
  - Iteration budget spent on note creation (9 notes = many tool calls)
  - The "curate web cache" step comes after notes, and by then the model is winding down

---

## Attempt 4a: Add todo planning step

**Hypothesis:** A checklist (via `todo` tool) would give the model structure and keep it
on track, like it does for deep-research.

**Changes:** Added `todo` to tool list, added "Plan your work" step before notes.

**Result:** REGRESSION — 3 notes, T2=PARTIAL. Model spent iteration budget planning
instead of executing. The todo tool is good for deep-research (open-ended, many cycles)
but counterproductive for reflection (bounded, mechanical).

---

## Attempt 4b: High-level todo (5-10 items)

**Hypothesis:** Maybe the todo was too granular. Make it high-level.

**Changes:** Softened todo instructions to "5-10 items, batch similar work."

**Result:** Same regression — 3 notes, T2=PARTIAL. Planning overhead still too high.

---

## Attempt 4c: Cache-first ordering (no todo)

**Hypothesis:** If we process cache before notes, references are in place for wiki-linking.

**Changes:** Removed `todo`, reordered to cache → notes.

**Result:** TIMEOUT at 3 minutes. Model started mechanically reading/curating cache files
one by one and never reached note creation.

---

## Attempt 5: Notes-first + self-check (current best)

**Hypothesis:** Attempt 3's ordering was correct (notes first). Adding a self-check step
before handoff might push the model to also curate references.

**Changes:**
- Restored notes-first ordering (matching attempt 3)
- Added self-check step: "Did you create entity notes? If not, go back. Did you call
  reference_manage for at least some files? If not, go back."
- Kept "batch similar files" hint for cache curation
- No todo tool

**Result:** T1=PASS, T2=PASS, T3=PASS — all tiers!

- **16 notes** created (vs 9 in attempt 3):
  - Entity notes: Bambu Lab P1S, Bambu Lab P2S, Prusa CORE One, Creality K2 Pro, QIDI Q2
  - Source quality note: Tom's Hardware
  - Decision note: Enclosed 3D Printer Purchase Decision ($800-$1200)
  - Topic hubs: 3dprinting, printers, enclosed-printers, buying-decisions
  - Topic note: Enclosed 3D Printers Around $1000 (2026)
- **1 reference** curated (Prusa CORE One product page)
- Completed in ~95 seconds (within 3-minute timeout)

**What worked:**
- Self-check step: model verified its work before handing off, caught the missing
  reference curation and did at least one
- Notes-first ordering: model creates rich content while it has budget, then curates
- Batch hint: model didn't try to read every cache file individually

**Remaining gap:**
- Only 1 of 25 cache files curated — self-check got it to do *something* but not
  thorough curation. This is acceptable for now (notes are the primary value).
- Question: would including tool results in transcript context help the model curate
  references without needing to read them individually?

---

## Attempt 6: Deterministic reference curation (spec 28)

**Hypothesis:** The agent can't reliably do both note-writing AND reference curation in
one pass. Across 11 attempts: when it writes great notes (13 entity notes), it skips
references. When it curates references well, it writes shallow notes. The two tasks
compete for the model's attention budget.

**New approach:** Let the agent focus on note-writing (what it's good at). Handle
reference curation deterministically in Rust code after the agent finishes.

**Changes:**
- `prompts/agents/reflection.md`:
  - Removed `reference_manage` from tools list entirely
  - Removed `reference_manage` progress nudge
  - Simplified workflow to 4 steps: Discover → Create notes → Verify → Handoff
  - Added source citation guidance: "Cite sources using `Source: <url>` lines"
  - Explicit: "Do NOT use `[[references/...]]` wiki links"
- `src/jobs/reflection.rs`:
  - Replaced `format_classified_cache()` output with structured XML (`<web-cache>`)
  - Added `curate_references()` post-processing: scans notes for source URLs,
    moves cited/URL-matched cache files to `references/{domain}/`, deletes rest
  - Session-scoped: only touches files from the captured `ClassifiedCacheFile` list
  - Replaced `clear_web_cache()` call with `curate_references()` in `run()`
- `tests/common.rs`: `run_reflection()` calls `curate_references()` after agent
- `tests/heartbeat_reflection.rs`: T3 promoted from aspirational to hard assert

**Result:** T1=PASS, T2=PASS, T3=PASS — first attempt!

- **5 notes** created:
  - Entity notes: Bambu Lab P1S, Prusa CORE One 2025, QIDI Q2
  - Topic hubs: 3dprinting, 3dprinting/printers
- **5 references** curated (deterministic — 100% reliable):
  - `references/tomshardware-com/` (P1S review)
  - `references/prusa3d-com/` (CORE One product page)
  - `references/bambulab-com/` (Bambu official)
  - `references/us-qidi3d-com/` (QIDI Q2 product page)
  - `references/auroratechchannel-com/` (price tracker)
- **20 files deleted** from web cache (search results, uncited pages)
- Completed in ~33 seconds (fastest yet — no reference_manage tool call overhead)

**What worked:**
- Deterministic curation: 5/5 cited files moved, 20/20 uncited deleted — zero ambiguity
- Agent freed from curation: focused entirely on note creation, still produced 3 entity
  notes with concrete specs and source URLs
- XML cache format: model could see sources and cite them in notes without needing to
  manage them
- Session scoping: only touched files from our classified list — safe for concurrent runs

**Key insight:** Splitting agent work (creative/judgment tasks) from mechanical work
(file moves/deletes) is strictly better. The agent's strength is synthesis; code's
strength is determinism. This pattern should apply to other reflection tasks too.

---

## Attempt 7: Structured sources, wiki links, quality improvements

**Hypothesis:** Note quality has specific gaps: (1) zero wiki links — no graph edges
created, (2) bare URLs in body instead of frontmatter, (3) empty index notes, (4)
source notes not domain-scoped, (5) agent findings not prioritized over raw cache data.

**Changes:**

Code:
- `src/knowledge/types.rs`: Added `sources: Vec<String>` to `NoteFrontMatter`
- `src/db/schema.rs`: Added `DEFINE FIELD sources ON note TYPE array<string> DEFAULT []`
- `src/db/knowledge/crud.rs`: Added `sources` param to `create_note_full` + `update_note`
- `src/tools/note_write.rs`: Added `sources` param to tool schema, wiki link hint when
  `wiki_links.is_empty() && archetype != "topic"`
- `src/knowledge/files.rs`: `ensure_index_notes` now creates body "Knowledge hub for
  {title}.\n" instead of empty string
- `src/jobs/reflection.rs`: `collect_urls_recursive` also extracts URLs from frontmatter
  `sources` field via `parse_note()`

Prompt (`prompts/agents/reflection.md`):
- Added "Agent findings are the primary source" paragraph in Step 2
- Added `sources` parameter usage instruction (replacing bare URL guidance)
- Added dedicated "Linking (critical)" subsection with concrete patterns
- Added "link UP to topic note" guidance for graph hubs
- Domain-scoped source note guidance (`3d-printing/sources` not `sources/3d-printing`)

**Result:** T1=PASS, T2=PASS, T3=PASS

- **6 notes** created:
  - Entity notes: Prusa CORE One, Bambu Lab P1S, QIDI Q2
  - Decision note: Enclosed 3D Printer Decision (2026-02, $800-$1200)
  - Source quality notes: Tom's Hardware, Aurora Tech Channel Price Tracker
- **5 references** curated (deterministic, same as attempt 6)
- **20 files** deleted from web cache
- Completed in ~49 seconds

**Quality checklist:**

| # | Check | Result | Evidence |
|---|-------|--------|----------|
| Q1 | Wiki links exist | PASS | Every entity note has 2+ links. Prusa: 7, P1S: 4, QIDI: 5, Decision: 8 |
| Q2 | Links meaningful | PASS | All targets are real entities: 3D Printing, Bambu Lab P1S, PrusaSlicer, MMU3, etc. |
| Q3 | Sources in frontmatter | PASS | All 6 notes have `sources = [...]` with real URLs |
| Q4 | No bare URLs in body | PASS | Zero bare URLs in any note body |
| Q5 | Source notes scoped | PASS | Tags are `3d-printing/sources` (2-level, domain-scoped) |
| Q6 | Topic notes non-empty | PASS | All index.md have body "Knowledge hub for {title}." |
| Q7 | Entity notes link up | PASS | All 3 entity notes start "Relevant to [[3D Printing]]" |
| Q8 | Agent findings priority | SOFT MISS | P1S note created instead of P2S. Decision note correctly reflects agent recommendation (Prusa top pick). P1S is a valid comparison target. |
| Q9 | Note content quality | PASS | Concrete specs: prices ($949/$1199, $699), volumes (256³, 270²×256), temps (370°C/120°C/65°C), speeds (500/600 mm/s) |

**Score: 8/9 PASS, 1 soft miss**

**What worked:**
- `sources` in frontmatter: model uses the parameter correctly, no bare URLs in body
- Wiki links: massive improvement — 28+ unique links across 6 notes, creating real graph
  edges. The "Linking (critical)" prompt section + wiki link hint worked.
- Domain-scoped source notes: `3d-printing/sources` tagging correct
- Index notes: non-empty bodies (minor but prevents empty embedding vectors)
- Entity notes link up: every entity note has `Relevant to [[3D Printing]]`
- Note quality: rich specs from review data, not vague summaries

**Q8 analysis:** The P1S vs P2S distinction is nuanced. The test fixture's agent
findings recommend CORE One as top pick and mention both P1S and P2S Bambu models.
The model chose P1S because the Tom's Hardware review (cached) has detailed P1S data.
The decision note's recommendations correctly follow the agent synthesis. This is an
acceptable outcome — the model prioritized documentable evidence over naming the newer
model without detailed specs.

**Verdict:** Quality bar met. All must-haves (Q1, Q3, Q8-adjusted, Q9) pass. Nice-to-haves
(Q5, Q7) also pass. Ready to commit.

---

## Attempt 8: Entity extraction step + `reference_manage` removal

**Hypothesis:** The model conflates related entities (P1S vs P2S) because it doesn't
enumerate agent-recommended entities before writing notes. It gravitates toward cached
data (P1S has a Tom's Hardware review) over agent recommendations (P2S is the top pick).
Adding an explicit enumeration sub-step forces the model to list every distinct entity
before creating notes.

Separately, `reference_manage` is dead code — curation is now deterministic in Rust.
Removed it everywhere.

**Changes:**

Code:
- Deleted `src/tools/reference_manage.rs` and all registrations/tests
- Simplified `note_write` warning message (no longer mentions `reference_manage`)
- Removed vacuous negative assertion in `definition.rs`

Prompt (`prompts/agents/reflection.md`):
- Added entity extraction sub-step before note creation: "Before writing any notes, list
  every distinct entity the agent findings explicitly named, recommended, or compared.
  Each one gets its own note — don't merge related items into a single note even if
  they're from the same family or manufacturer."

Test:
- Promoted P2S entity note from soft T2 check to hard assert

**Result:** T1=PASS, T2=PASS, T3=PASS — first try (~50s)

- **11 notes** created:
  - Entity notes: Prusa CORE One, **Bambu P2S** (new!), Bambu P1S, Creality K2 Pro, QIDI Q2
  - Source quality: Tom's Hardware — Bambu P1S Review
  - Decision: Enclosed 3D Printer Selection 2026
  - Index/hub: 3dprinting, printers, sources, decisions
- **7 references** curated (deterministic), **18 deleted**
- **17 cited edges** created
- P2S note: concrete pricing ($799 street), AMS mention, trade-offs vs Prusa CORE One,
  wiki links to [[Bambu Lab]], [[AMS]], [[Prusa CORE One]], [[Enclosed 3D Printer
  Selection 2026]]

**What worked:**
- Entity extraction step: model explicitly listed all agent-recommended entities before
  writing, preventing the P1S/P2S conflation
- P2S and P1S created as separate notes with distinct content (P2S focuses on recency and
  value positioning, P1S on review evidence and established ecosystem)
- `reference_manage` removal had zero impact — deterministic curation handles everything

**Key insight:** Forcing enumeration before execution breaks the "write about whatever has
the most data" bias. The model's default is to start writing about the entity it knows
most about; an explicit listing step makes it commit to covering all entities first.

---

## Attempt 9: Generic prompt (first pass — too soft)

**Hypothesis:** The prompt is over-fitted to product-research reflections. Terms like
"Product X", "manufacturer", "prices, dimensions", hard minimums ("at least 3 entity
notes"), and assumed presence of agent findings/web cache make it unsuitable for plain
chat reflections or non-research agent sessions.

**Changes to `prompts/agents/reflection.md`:**
- Replaced "Agent findings are the primary source" with conditional: "If an Agent
  Findings section is present... If web cache files are present... For plain
  conversations without either, extract knowledge directly from the transcript."
- Replaced "product/tool/service" entity language with generic: "person, project,
  concept, tool, or other concrete entity"
- Replaced hard minimums in Step 3 ("at least 3 entity notes", "at least 1 source
  quality note") with soft checks: "Did you create a note for every distinct entity?"
  and "If external sources were used, did you create at least one source quality note?"
- Replaced "from the same family or manufacturer" with "closely related or from the same
  category"
- Nudge message: "from the agent findings" → "from the conversation"

**Result:** T1=PASS, T2=PASS, T3=PASS — but **quality regression**

- **7 notes** (down from 11-12): Prusa CORE One, QIDI Q2, plus hubs
- **No decision note**, **no source quality note**
- Handoff explicitly says "I did not add a decision or source quality note"
- Entity extraction still works (P2S created via duplicate-safe block)

**Diagnosis:** The softer verification step ("did you...?") lets the model skip work it
considers optional. The hard minimums were product-specific in wording but correct in
intent — they forced thoroughness.

---

## Attempt 10: Generic prompt + stronger verification (current best)

**Hypothesis:** Keep the generic wording but strengthen the self-check to reference the
entity list from step 2 and explicitly require decision/source notes when applicable.

**Changes:**
- Step 3 verification now says: "check your work against the entity list from step 2"
- "Did you create or confirm a note exists for **every** entity you listed?"
- Added explicit decision note check: "If comparisons or trade-offs were discussed, did
  you create a decision note?"

**Result:** T1=PASS, T2=PASS, T3=PASS

- **12 notes** created: Prusa CORE One, Creality K2 Pro, QIDI Q2, Tom's Hardware source
  note, Aurora Tech Channel source note, Enclosed 3D Printer Selection decision note,
  plus Bambu P2S/P1S (confirmed existing), plus index/hub notes
- **7 references** curated, **18 cited edges**, **18 deleted** from cache
- Source quality notes: 2 (Tom's Hardware + Aurora Tech Channel)
- Decision note: present and comprehensive

**What worked:**
- Generic language: no product/manufacturer terminology, works for any domain
- Conditional input handling: agent findings, web cache, and plain transcript all covered
- Verification references the entity list: model checks its own enumeration, catches gaps
- Explicit decision/source checks: model creates them when the input warrants it

**Key insight:** Generic prompts need stronger self-check steps, not weaker ones. The
previous hard minimums worked because they forced action, not because they were
product-specific. The fix: make the verification reference the model's own entity list
(dynamic, not fixed numbers) and explicitly mention decision/source notes as conditional
requirements.
