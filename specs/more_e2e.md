We implemented true e2e tests that actually run the daemon

Current command:

```sh
cargo test --features live-tests test_ark_nova_import -- --nocapture
```

We need to:

- Create a feature flag specific to those
- Add more scenarios, testing the coding agent, job creation, agent creation... All in
  all, it should cover all features and all skills
