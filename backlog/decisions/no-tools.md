## All CLI

I tried removing as many tools as possible and getting more into the ghost cli:

- Models spend a lot of time reading doc (`ghost web --help`, ...)
- GPT 5.3 had issues batching multiple calls, ended up trying to do them into one huge
  command, whereas with tools there is natural separation
