At this point, we need a small refactor:

- All prompts should be text files in /prompts
- ANYTHING THAT GETS PASSED TO A MODEL SHOULD BE IN /prompts

Then we need full human user validation

---

Check all mod.rs files to make sure they don't hold application code
