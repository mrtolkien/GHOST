Create a robust message passing system:

- Agents -> GHOST
- Background shell -> GHOST
- Jobs, ...
- Steering messages -> tool loop, ...
- Finish import -> chunk + embed
- File watcher -> chunk + embed (but wait if file is already being worked on, thanks to
  a message dispatch during the import)

Using a real message queue with pub/sub many-to-many for low coupling might make the
code simpler?

Not sure really
