#!/usr/bin/env python3
"""Minimal SPIR-V disassembler for Harpia .spv files.

`assemble_spvasm.py` writes SPIR-V by hand, so when spirv-val rejects a module
with an unhelpful message this prints the instruction stream to find it. Only
knows the opcodes the assembler emits; anything else shows as `Op<number>`.
"""
from __future__ import annotations

import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from assemble_spvasm import OP  # noqa: E402

NAMES = {v: k for k, v in OP.items()}


def main() -> int:
    data = Path(sys.argv[1]).read_bytes()
    words = list(struct.unpack(f"<{len(data) // 4}I", data))
    if words[0] != 0x07230203:
        print("not SPIR-V")
        return 1
    print(f"version {words[1] >> 16 & 0xFF}.{words[1] >> 8 & 0xFF}  bound {words[3]}")
    i = 5
    while i < len(words):
        count = words[i] >> 16
        opcode = words[i] & 0xFFFF
        if count == 0:
            print(f"!! word count 0 at word {i}")
            return 1
        operands = words[i + 1 : i + count]
        name = NAMES.get(opcode, f"Op{opcode}")
        print(f"{i:5}  {name:32} {' '.join(str(o) for o in operands)}")
        i += count
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
