Reloading config at runtime will help for a lot of features:

- Simpler to add a provider through the GHOST itself
- Simpler to add a browser
- Less chat break

It should not be that hard to have a "ghost config reload" command that invalidates the
config in memory and reads the new one, but we need to make sure this properly
propagates everywhere.
