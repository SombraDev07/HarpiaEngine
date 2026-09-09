# 3rdPartyLibs

Vendor C/C++ e SDKs. Cada pasta traz o código **e** o `LICENSE` original.

Nada daqui entra em `prog/engine/`. Wrappers Rust que chamam estes SDKs vivem em `prog/plugins/` (cdylib) e falam com a engine pelo trait RHI ou por declaração de pass — **nunca** com `vkCmd*`.

## Agora

Vazio de propósito. Hello-triangle não precisa de vendor.

## Depois (não copiar antes da fase)

| Pasta | Quando | Uso |
|---|---|---|
| FidelityFX SPD (nome = o do SDK) | Bloom | Single Pass Downsampler. **Não** reescrever SPD. |
| FSR | Depois de TAA verde | Upscale. **Não** antes. |
| meshoptimizer | Se o crate `meshopt` não chegar | Preferir crates.io. |

FidelityFX **nunca** vive dentro de `harpia-render`.
