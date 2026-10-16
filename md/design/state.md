# State

Documents the kind of state maintained by the Symposium agent.

`state.toml` in the config directory holds what Symposium records about the machine rather than what the user configures: the version of the binary that last ran, when it last checked for updates, and the agents the last `cargo agents init` detected. `init` uses that last list to tell agents installed since then (pre-checked) from agents the user already saw and deselected (left unchecked).
