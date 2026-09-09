# Memory index

Persistência entre sessões da IA. **Não** é o allocator (`prog/engine/memory`).

## Ordem de leitura (início de sessão)

1. Este ficheiro
2. `PROGRESS.md` — fase actual, o que está verde, o que falta
3. `DECISIONS.md` — decisões fechadas (não reabrir sem motivo)
4. `LANDMINES.md` — se fores mexer em GPU / swapchain / barreiras / AMD

## O que não vive aqui

- Código
- Doutrina (`docs/Rust-Rewrite-*.md`)
- Layout bindless (`docs/Bindless-Descriptor-Layout.md`)

## Fim de sessão

Actualizar `PROGRESS.md`. Acrescentar decisões e minas novas. Sem isto a sessão não fechou.
