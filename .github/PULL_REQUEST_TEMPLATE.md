# What & why

<!-- What changed, and why it changed. Decisions worth recording go here, not just the diff restated. -->

# Testing performed

<!-- What was run and where. Name the platforms: Windows / Linux, and note anything only verifiable locally. -->

# Windows / Linux considerations

<!-- Anything platform-specific: behavior differences, seam changes, CI impact. Write "none" if genuinely none. -->

# Screenshots

<!-- UI changes: before/after for the affected states. -->

# Performance impact

<!-- Required if this touches a hot path: log pipeline, IPC, stream delivery, fs operations. State what you measured. Otherwise "none". -->

# Checklist

- [ ] Conventional commit title; scope matches the change
- [ ] Glossary checked — no new synonyms for existing concepts; new terms added to GLOSSARY.md
- [ ] No unrelated changes in this PR
- [ ] Architecture-affecting? Linked ADR created/updated
- [ ] Safety invariants respected (process identity, fs containment, bounded queues) — nothing deferred to "later hardening"
- [ ] Tests: behavior change covered; lifecycle/security matrix unaffected or extended
- [ ] Docs/comments follow the style guide; no narrating comments
- [ ] CI green on both platforms
