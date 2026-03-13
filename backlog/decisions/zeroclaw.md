# Extending ZeroClaw

https://github.com/zeroclaw-labs/zeroclaw

ZeroClaw does all the busywork really well for what we need:

- Tons of providers and interfaces
- Well abstracted
- Handles embedddings
- ...

_But_:

- It is not exposed as a crate, we cannot simply extend it to be bigger but more capable
  (cf our design with lua agents, web fetch and search, ...)
- Forking it is not a great idea for such an early project

## Conclusion

We'll keep ZeroClaw as an inspiration for design and features implementation: it's a
great example, already widely used, and in Rust. From now on, it is our #1 source of
inspiration.
