Aider-style structural repo map for the coding agent.

## What It Does

Gives the coding agent a compact, ranked structural overview of the entire repo --
injected into its prompt every turn. The agent sees function signatures, class
definitions, and type declarations without needing to search or read files first.

Complementary to embeddings (semantic search): the repo map tells the agent _what
exists_, embeddings help it _find semantically related code_.

## How It Works

1. **Parse**: Tree-sitter extracts symbol definitions (functions, classes, structs,
   traits) and references (calls, usages) from every file
2. **Graph**: Builds a NetworkX directed graph -- files are nodes, edges are "file A
   references symbol defined in file B"
3. **Rank**: PageRank with personalization -- files mentioned in chat or containing
   identifiers from the user's message get boosted. Clever weighting:
   - snake_case identifiers >= 8 chars: 10x (domain-specific names matter)
   - Private `_` identifiers: 0.1x
   - Symbols defined in >5 files: 0.1x (too generic)
   - Files in active chat: 50x
4. **Render**: Top-ranked definitions shown with `⋮` elision markers (just signatures +
   parent scope), fitted within a token budget (default 1K, expands to 8K on first turn)
5. **Cache**: Tree-sitter parsing is disk-cached per file (mtime-based). The map itself
   is regenerated every turn because the user's message changes which identifiers are
   boosted.

## Example of Injected Context

This is the actual text the LLM sees in its system prompt:

```
aider/coders/base_coder.py:
⋮
│class Coder:
│    abs_fnames = None
⋮
│    @classmethod
│    def create(
│        self,
│        main_model,
│        edit_format,
│        io,
│        skip_model_availabily_check=False,
│        **kwargs,
⋮
│    def abs_root_path(self, path):
⋮
│    def run(self, with_message=None):
⋮

aider/commands.py:
⋮
│class Commands:
│    voice = None
⋮
│    def get_commands(self):
⋮
│    def get_command_completions(self, cmd_name, partial):
⋮
│    def run(self, inp):
⋮
```

Files already fully loaded into the chat are excluded from the map (the LLM already sees
their content). The `⋮` markers show where code was elided. The `│` prefix marks lines
of interest (definitions, signatures).

## References

- Docs: https://aider.chat/docs/repomap.html
- Blog: https://aider.chat/2023/10/22/repomap.html
- Source: https://github.com/Aider-AI/aider/blob/main/aider/repomap.py
- Deep dive: https://deepwiki.com/Aider-AI/aider/4.1-repository-mapping
