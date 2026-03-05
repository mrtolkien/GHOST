# Skill Authoring Best Practices

> Practical guidance for writing effective skills that agents can discover and use
> successfully.

Good skills are concise, well-structured, and tested with real usage. This guide
provides practical authoring decisions to help you write skills that work effectively.

## Core Principles

### Concise is key

The context window is a shared resource. Your skill shares it with everything else the
agent needs: system prompt, conversation history, other skills' metadata, and the actual
request.

Not every token has an immediate cost. At startup, only the metadata (name and
description) from all skills is pre-loaded. The agent reads skill.md only when the skill
becomes relevant, and reads additional files only as needed. However, being concise in
skill.md still matters: once loaded, every token competes with conversation history and
other context.

**Default assumption**: The agent is already very smart

Only add context the agent doesn't already have. Challenge each piece of information:

- "Does the agent really need this explanation?"
- "Can I assume the agent knows this?"
- "Does this paragraph justify its token cost?"

**Good example: Concise** (approximately 50 tokens):

````markdown
## Extract PDF text

Use pdfplumber for text extraction:

```python
import pdfplumber

with pdfplumber.open("file.pdf") as pdf:
    text = pdf.pages[0].extract_text()
```
````

**Bad example: Too verbose** (approximately 150 tokens):

```markdown
## Extract PDF text

PDF (Portable Document Format) files are a common file format that contains text,
images, and other content. To extract text from a PDF, you'll need to use a library.
There are many libraries available for PDF processing, but we recommend pdfplumber
because it's easy to use and handles most cases well. First, you'll need to install it
using pip. Then you can use the code below...
```

The concise version assumes the agent knows what PDFs are and how libraries work.

### Set appropriate degrees of freedom

Match the level of specificity to the task's fragility and variability.

**High freedom** (text-based instructions):

Use when:

- Multiple approaches are valid
- Decisions depend on context
- Heuristics guide the approach

**Medium freedom** (pseudocode or scripts with parameters):

Use when:

- A preferred pattern exists
- Some variation is acceptable
- Configuration affects behavior

**Low freedom** (specific scripts, few or no parameters):

Use when:

- Operations are fragile and error-prone
- Consistency is critical
- A specific sequence must be followed

**Analogy**: Think of the agent as exploring a path:

- **Narrow bridge with cliffs**: Only one safe way forward. Provide specific guardrails
  and exact instructions (low freedom). Example: database migrations.
- **Open field with no hazards**: Many paths lead to success. Give general direction and
  trust the agent (high freedom). Example: code reviews.

### Test with real usage

Skills act as additions to models, so effectiveness depends on the underlying model.
Test your skill with real scenarios to verify it works as intended.

## Skill Structure

### Naming conventions

Use consistent naming patterns. We recommend **gerund form** (verb + -ing) for skill
names, as this clearly describes the activity:

**Good naming examples:**

- "condition-based-waiting" not "async-test-helpers"
- "creating-skills" not "skill-creation"
- "systematic-debugging" not "debug-tools"

**Avoid:**

- Vague names: "Helper", "Utils", "Tools"
- Overly generic: "Documents", "Data", "Files"

### Writing effective descriptions

The `description` field enables skill discovery and should include both what the skill
does and when to use it.

**Always write in third person.** The description is injected into the system prompt.

- **Good:** "Processes Excel files and generates reports"
- **Avoid:** "I can help you process Excel files"

**Be specific and include key terms.** The description is critical for skill selection:
the agent uses it to choose the right skill from potentially many available skills.

Effective examples:

```yaml
description:
  Extract text and tables from PDF files, fill forms, merge documents. Use when working
  with PDF files or when the user mentions PDFs, forms, or document extraction.
```

```yaml
description:
  Generate descriptive commit messages by analyzing git diffs. Use when the user asks
  for help writing commit messages or reviewing staged changes.
```

Avoid vague descriptions like: "Helps with documents", "Processes data"

### Progressive disclosure patterns

skill.md serves as an overview that points the agent to detailed materials as needed,
like a table of contents.

**Practical guidance:**

- Keep skill.md body under 500 lines for optimal performance
- Split content into separate files when approaching this limit
- Use the patterns below to organize instructions, code, and resources effectively

#### Pattern 1: High-level guide with references

````markdown
---
name: PDF Processing
description:
  Extracts text and tables from PDF files, fills forms, and merges documents. Use when
  working with PDF files or when the user mentions PDFs, forms, or document extraction.
---

# PDF Processing

## Quick start

Extract text with pdfplumber:

```python
import pdfplumber
with pdfplumber.open("file.pdf") as pdf:
    text = pdf.pages[0].extract_text()
```

## Advanced features

**Form filling**: See read_file("skills/pdf/forms.md") for complete guide **API
reference**: See read_file("skills/pdf/reference.md") for all methods
````

The agent loads companion files only when needed.

#### Pattern 2: Domain-specific organization

For skills with multiple domains, organize content by domain to avoid loading irrelevant
context.

```
bigquery-skill/
  skill.md (overview and navigation)
  reference/
    finance.md (revenue, billing metrics)
    sales.md (opportunities, pipeline)
    product.md (API usage, features)
```

### Avoid deeply nested references

The agent may partially read files when they're referenced from other referenced files.

**Keep references one level deep from skill.md.** All reference files should link
directly from skill.md.

### Structure longer reference files with table of contents

For reference files longer than 100 lines, include a table of contents at the top. This
ensures the agent can see the full scope of available information even when previewing.

## Workflows and Feedback Loops

### Use workflows for complex tasks

Break complex operations into clear, sequential steps. For particularly complex
workflows, provide a checklist the agent can track progress with using
`todo(action: "plan", ...)`.

### Implement feedback loops

**Common pattern**: Run validator -> fix errors -> repeat

This pattern greatly improves output quality.

## Content Guidelines

### Avoid time-sensitive information

Don't include information that will become outdated. Use "current method" vs "old
patterns" sections instead of date-based conditionals.

### Use consistent terminology

Choose one term and use it throughout the skill:

**Good - Consistent:**

- Always "API endpoint"
- Always "field"
- Always "extract"

**Bad - Inconsistent:**

- Mix "API endpoint", "URL", "API route", "path"
- Mix "field", "box", "element", "control"

## Common Patterns

### Template pattern

Provide templates for output format. Match the level of strictness to your needs.

### Examples pattern

For skills where output quality depends on seeing examples, provide input/output pairs.
Examples help the agent understand the desired style and level of detail more clearly
than descriptions alone.

### Conditional workflow pattern

Guide the agent through decision points with clear branching logic.

## Evaluation and Iteration

### Build evaluations first

**Create evaluations BEFORE writing extensive documentation.** This ensures your skill
solves real problems rather than documenting imagined ones.

**Evaluation-driven development:**

1. **Identify gaps**: Run agent on representative tasks without a skill. Document
   specific failures or missing context
2. **Create evaluations**: Build three scenarios that test these gaps
3. **Establish baseline**: Measure agent's performance without the skill
4. **Write minimal instructions**: Create just enough content to address the gaps
5. **Iterate**: Execute evaluations, compare against baseline, and refine

### Develop skills iteratively

The most effective skill development process involves agents themselves. Work with one
instance ("Agent A") to create a skill that will be used by other instances ("Agent B").
Agent A helps you design and refine instructions, while Agent B tests them in real
tasks.

**Creating a new skill:**

1. **Complete a task without a skill**: Work through a problem with Agent A. Notice what
   information you repeatedly provide.
2. **Identify the reusable pattern**: After completing the task, identify what context
   would be useful for similar future tasks.
3. **Ask Agent A to create a skill**: "Create a skill that captures this pattern."
4. **Review for conciseness**: Check that Agent A hasn't added unnecessary explanations.
5. **Improve information architecture**: Ask Agent A to organize content effectively.
6. **Test on similar tasks**: Use the skill with Agent B on related use cases.
7. **Iterate based on observation**: If Agent B struggles, return to Agent A with
   specifics.

### Observe how the agent navigates skills

Watch for:

- **Unexpected exploration paths**: Does the agent read files in an order you didn't
  anticipate?
- **Missed connections**: Does the agent fail to follow references?
- **Overreliance on certain sections**: Consider moving that content to skill.md
- **Ignored content**: Might be unnecessary or poorly signaled

## Anti-Patterns to Avoid

### Avoid offering too many options

Don't present multiple approaches unless necessary. Provide a default with an escape
hatch for edge cases.

### Avoid Windows-style paths

Always use forward slashes in file paths: `reference/guide.md`, not `reference\guide.md`

## Advanced: Skills with Executable Code

### Solve, don't punt

Handle error conditions rather than leaving them for the agent to figure out.

### Provide utility scripts

Pre-made scripts are more reliable than generated code, save tokens, save time, and
ensure consistency.

### Create verifiable intermediate outputs

Use the "plan-validate-execute" pattern: create plan file -> validate plan -> execute ->
verify. This catches errors early with machine-verifiable validation.

## Checklist for Effective Skills

Before deploying a skill, verify:

### Core quality

- [ ] Description is specific and includes key terms
- [ ] Description includes both what the skill does and when to use it
- [ ] skill.md body is under 500 lines
- [ ] Additional details are in separate files (if needed)
- [ ] No time-sensitive information
- [ ] Consistent terminology throughout
- [ ] Examples are concrete, not abstract
- [ ] File references are one level deep
- [ ] Progressive disclosure used appropriately
- [ ] Workflows have clear steps

### Code and scripts

- [ ] Scripts solve problems rather than punt to agent
- [ ] Error handling is explicit and helpful
- [ ] No magic numbers (all values justified)
- [ ] Required packages listed and verified
- [ ] No Windows-style paths (all forward slashes)
- [ ] Validation/verification steps for critical operations
- [ ] Feedback loops included for quality-critical tasks

### Testing

- [ ] At least three evaluation scenarios created
- [ ] Tested with real usage scenarios
- [ ] Team feedback incorporated (if applicable)
