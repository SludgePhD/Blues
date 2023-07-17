# BlueZ bindings without the fuss

Or rather, with my kind of fuss.

Goals:
- don't require tokio (there's an optional feature for tokio support you can enable)
- don't expose unstable dependencies in public API (including `#[async_trait]`, the `futures` crate, and `zbus`)
  - in fact the public API exposes *nothing* that isn't available in libstd
- don't have unreasonably many dependencies
  - this library doesn't currently deliver on this goal, `zbus` pulls in a *lot* of dependencies
