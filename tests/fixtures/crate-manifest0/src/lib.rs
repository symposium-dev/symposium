// manifest-host depends on crate-m; a vouch plugin loads crate-m's plugin
// through a `[[plugins]]` chained reference. crate-m ships its own
// SYMPOSIUM.toml, so it loads as a first-class plugin (not the default
// `skills/` convention).
