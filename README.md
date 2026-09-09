# Harpia

Engine 3D em Rust. Vulkan 1.3 (`ash` + `gpu-allocator`). Código em `prog/`, doutrina em `docs/`, memória da IA em `memory/`.

**Não** é um port da Tucano C++. A Tucano é referência a superar (PBR + IBL + fog + clima + bindless honestos).

## Gates

```bash
# Fase 1 — triângulo, 90 frames, validation 0 (default)
cargo run -p hello-triangle -- --frames 90

# Sem GPU
cargo run -p hello-triangle -- --backend null --frames 8

# Janela até fechar (opt-in; no AMD/RADV não é o default)
cargo run -p hello-triangle -- --interactive
```

`MESA_VK_ABORT_ON_DEVICE_LOSS=1` é definido pelo loop. Validation ON. Sem `--frames` unbounded.

## Árvore

```
prog/engine/{core,memory,math,drv,app,plugin,…}
prog/{samples,tools,plugins,1stPartyLibs,3rdPartyLibs}
```

`drv` é o único crate com `vk::*`.
