Create a robust message passing system:

- Agents -> GHOST
- Background shell -> GHOST
- Jobs, ...
- Steering messages -> tool loop, ...
- Finish import -> chunk + embed
- File watcher -> chunk + embed (but wait if file is already being worked on, thanks to
  a message dispatch during the import)
- Almost all CLI calls would simply become messages sent to the running daemon, which
  would also open the door for a remote CLI

Using a real message queue with pub/sub many-to-many for low coupling might make the
code simpler?

The daemon would then:

- Run a message queue (there has to be a good Rust-native one)
- Hold all the subscribers to it
- Allow some remote producers through the CLI for example

Not sure really
