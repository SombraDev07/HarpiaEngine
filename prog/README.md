# prog/

Código. Doutrina em `docs/`. Memória da IA em `memory/`.

Um módulo = uma pasta. O `Cargo.toml` do crate vive **nessa** pasta (`name = "harpia-core"`, path `engine/core`). Sem `engine/core/harpia-core`.

```
prog/
  engine/
    core/ memory/ math/
    drv/                 # RHI — único vk::*
    render/ shaders/ weather/ terrain/ gi/
    scene/ world/ vegetation/
    app/ plugin/ assets/
    editor/
  1stPartyLibs/  3rdPartyLibs/  plugins/  samples/  tools/
```
