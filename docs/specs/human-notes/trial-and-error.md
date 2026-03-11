## Surrealdb

Many RAM usage issues w/ vectors + surrealkv

Also bugs in surreal 3

## Relfection as fork

- Better quality notes + cache hits
- BUT context pressure: usually deep research gets close to filling context...
- Also a bit more complex if we use CLI for note creation over tools

## All CLI

I tried removing as many tools as possible and getting more into the ghost cli:

- Models spend a lot of time reading doc (`ghost web --help`, ...)
- GPT 5.3 had issues batching multiple calls, ended up trying to do them into one huge
  command, whereas with tools there is natural separation
