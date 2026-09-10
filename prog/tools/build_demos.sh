#!/usr/bin/env bash
# Build every sample in release and collect them into demos/ under names that say
# what each one is for.
#
# The gates are already one-feature-each by design -- that is what a gate is --
# so this is packaging, not new code. Numbered so the order matches the roadmap
# phases: run them in order and you watch the engine get built.
set -euo pipefail

# cargo may not be on a non-login shell PATH (this is how CI and cron see it).
export PATH="$HOME/.cargo/bin:$PATH"

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
out="$root/demos"
cd "$root"

echo "==> cargo build --release --workspace"
cargo build --release --workspace

mkdir -p "$out"

# demo name              cargo package     what it is for
demos=(
  "01-triangulo:hello-triangle:Swapchain, dynamic rendering, resize. O mínimo que prova que a GPU responde."
  "02-bindless:gate-bindless:Heap bindless de 8192 slots, upload de mips, UAV de compute."
  "03-luz-pbr:gate-pbr-grid:Esferas: BRDF, sol, IBL split-sum, energy compensation. É aqui que se julga a luz."
  "04-sombras:gate-csm:Esferas + chão: 4 cascatas, atlas 2x2, PCSS. É aqui que se julga a sombra."
  "05-taa:gate-taa:Motion vectors + history + neighbourhood clamp."
  "06-fog-feixes:gate-fog:Froxels 160x90x64 e sombras volumétricas. Os feixes vêm do CSM no inject."
  "07-ceu:gate-sky:Hillaire 2020: transmittance -> multiscattering -> sky-view."
  "08-nuvens:gate-clouds:Raymarch Nubis a meia resolução + reprojecção temporal."
  "09-agua:gate-water:Gerstner, Fresnel, SSR e espuma."
  "10-chuva:gate-rain:Material molhado, ondulações e bátegas em espaço de ecrã."
  "sponza:sponza:A cena de referência: glTF, CSM+PCSS, fog default-on, chuva mascarada pelo mapa de chuva."
)

for entry in "${demos[@]}"; do
  name="${entry%%:*}"
  rest="${entry#*:}"
  pkg="${rest%%:*}"
  src="$root/target/release/$pkg"
  if [[ ! -x "$src" ]]; then
    echo "  FALTA: $pkg (esperado em $src)" >&2
    exit 1
  fi
  cp -f "$src" "$out/$name"
  echo "  $name  <-  $pkg"
done

cat > "$out/README.md" <<'EOF'
# Demos da Harpia

Binários de release, um por funcionalidade. Reconstrói com
`prog/tools/build_demos.sh` — esta pasta está no `.gitignore`, os binários não
são commitados.

## Correr

Cada demo aceita as mesmas opções:

    ./03-luz-pbr                 # 16 frames e sai (o modo de gate)
    ./03-luz-pbr --frames 240    # mais tempo para olhar
    ./03-luz-pbr -i              # interactivo, fecha na janela
    ./03-luz-pbr --capture /tmp/x   # PNG de cada alvo depois do último frame
    ./03-luz-pbr --backend null --frames 8   # sem GPU, para CI

**Aviso do host:** em AMD/RADV não deixes correr sem `--frames` durante horas.

## Câmara

`sponza`, `09-agua` e `10-chuva` voam: **WASD** move, **Q/E** sobe e desce,
**Shift** acelera, **botão direito** ou as **setas** olham, a **roda** ajusta a
velocidade. Os restantes têm câmara fixa ou animada de propósito.

Sob `--frames N` o input é **ignorado** e o passo de tempo é fixo (1/60), para
que uma captura seja sempre a mesma imagem. Só `-i` tem relógio e input a sério.

## O que cada um serve para julgar

| Demo | Julga |
|---|---|
| `01-triangulo` | swapchain, dynamic rendering, resize |
| `02-bindless` | heap bindless, upload de mips, UAV de compute |
| `03-luz-pbr` | **luz**: BRDF, sol, IBL, energy compensation |
| `04-sombras` | **sombra**: cascatas, snap de texels, PCSS |
| `05-taa` | motion vectors, history, clamp |
| `06-fog-feixes` | froxels e sombras volumétricas |
| `07-ceu` | atmosfera Hillaire |
| `08-nuvens` | raymarch e reprojecção temporal |
| `09-agua` | Gerstner, Fresnel, SSR, espuma |
| `10-chuva` | material molhado, ondulações, bátegas |
| `sponza` | **integração**: tudo junto numa cena a sério |

`sponza` precisa dos assets: `python3 prog/tools/fetch_sponza.py` (o glTF está no
`.gitignore`).
EOF

echo "==> $out pronto ($(ls -1 "$out" | wc -l) ficheiros)"
