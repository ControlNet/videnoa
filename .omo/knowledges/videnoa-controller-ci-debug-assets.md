# Controller Debug Asset Contract

Debug `videnoa-controller` resolves `controller-web/dist` from `CARGO_MANIFEST_DIR` and validates it before listener bind. The Controller build script builds or validates frontend assets only for release profiles, so every clean-checkout CI job that launches the debug binary must build `controller-web` in that same job. Process-level startup tests must retain child stderr so pre-bind configuration, asset, database, and listener errors remain distinguishable.
