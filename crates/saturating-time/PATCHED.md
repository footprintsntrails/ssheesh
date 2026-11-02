# Why this copy

This is `saturating-time` 0.5.0 from [Arti](https://gitlab.torproject.org/tpo/core/arti), with one fix in `find_limit` (`src/internal.rs`), and its tests made to pass on Windows.

On Windows, `SystemTime` counts in 100 ns, so subtracting 1 ns changes nothing and still succeeds. `find_limit` took that as progress and never ended, and Arti calls it as it parses the list of relays, so Tor hung at 15% on every Windows computer. Remove this copy, and the patch in the top `Cargo.toml`, once Arti has the fix.
