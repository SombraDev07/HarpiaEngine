# 3rdPartyLibs

Vendor C/C++ e SDKs. Cada pasta traz o código **e** o `LICENSE` original.

Nada daqui entra em `prog/engine/`. Wrappers Rust que chamam estes SDKs vivem em `prog/plugins/` (cdylib + rlib) e falam com a engine pelo trait RHI ou por SPIR-V + setup CPU — **nunca** com `vkCmd*`. Hello-triangle **não** dlopen nada.

## Agora

| Pasta | Uso |
|---|---|
| `ffx-spd/` | FidelityFX Single Pass Downsampler (`ffx_a.h`, `ffx_spd.h`). Bloom Karis + Hi-Z min. GPUOpen-Effects, MIT. |
| `ffx-sssr/` | FidelityFX SSSR (`ffx_sssr.h` + tradução GLSL). Marcha hierárquica. **Sem** DNSR neste slice. |

O host C++ da AMD (`ffxSpdContextDispatch`, etc.) **não** está aqui: o backend deles emite Vulkan, e `vkCmd*` fica em `harpia-rhi`.

## Depois (não copiar antes da fase)

| Pasta | Quando | Uso |
|---|---|---|
| FSR | Depois de TAA verde **e** do exit da fase 7 | Upscale. TAA já está verde; o exit da 7 ainda não. |
| meshoptimizer | Se o crate `meshopt` não chegar | Preferir crates.io. |

FidelityFX **nunca** vive dentro de `harpia-render`.
