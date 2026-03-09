Need to add more _true_ e2e tests:

- Need to update the e2e testing skill to differentiate between the old (linear) and new
  (daemon) e2e tests
- Need to write a few scenarios

Current command:

```sh
cargo test --features live-tests test_ark_nova_import -- --nocapture
```
