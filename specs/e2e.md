We should have a _true_ e2e test which actually starts the daemon and interacts with it.
The only cheating we would do is the provider/Discord layer: we would need to be able to
directly insert messages to the chat.

We need to use an LLM to role-play the user with a few pre-determined roles, and we
should use the LLM to evaluate if the test passed or not.
