# 1stPartyLibs

Libs **Harpia** que não são um módulo de `prog/engine/` (utilitários partilháveis, cook helpers, etc.).

Agora: vazio. Fullscreen blit vive em `prog/engine/render/shaders/` (`fullscreen.vs.glsl`,
`blit.ps.glsl`) — é o sítio certo, não um crate extra.

Não ponhas FidelityFX aqui — isso é `3rdPartyLibs/`. Não ponhas o RHI aqui — isso é `engine/drv/`.
