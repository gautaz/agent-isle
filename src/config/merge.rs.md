# `config/merge.rs` — Deep config merge

Performs a deep merge of two `Config` values.
Scalars are replaced, lists (applies, mounts) are appended, environment maps are merged with override semantics, and the tools config is deep-merged per section.
