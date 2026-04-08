# cargo-agents — AI the Rust Way

cargo-agents helps agents write better Rust by providing up-to-date language guidance and integration with the Rust ecosystem.

## Guidance on a particular crate

Before authoring Rust code that uses a particular crate, $INVOKE(crate,$name) will provide you with a path to the crate source, custom instructions for that crate, and a list of available skills that can be loaded.

## Skills available for current dependencies

The custom skills available for the dependencies currently found in the workspace are included below. You can read the skill file to learn more about it.

To display an updated list of skills, for example if new crates are added, invoke $INVOKE(crate,$name).
