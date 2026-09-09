#!/usr/bin/env python3
"""Minimal SPIR-V assembler for the hello-triangle .spvasm files. Prefer spirv-as / DXC when present."""
from __future__ import annotations

import re
import struct
import sys
from pathlib import Path

OP = {
    "OpCapability": 17,
    "OpMemoryModel": 14,
    "OpEntryPoint": 15,
    "OpExecutionMode": 16,
    "OpName": 5,
    "OpDecorate": 71,
    "OpTypeVoid": 19,
    "OpTypeFunction": 33,
    "OpTypeInt": 21,
    "OpTypeFloat": 22,
    "OpTypeVector": 23,
    "OpTypePointer": 32,
    "OpVariable": 59,
    "OpConstant": 43,
    "OpConstantComposite": 44,
    "OpFunction": 54,
    "OpFunctionEnd": 56,
    "OpLabel": 248,
    "OpLoad": 61,
    "OpStore": 62,
    "OpReturn": 253,
    "OpSelectionMerge": 247,
    "OpSwitch": 251,
    "OpBranch": 249,
    "OpCompositeExtract": 81,
    "OpCompositeConstruct": 80,
}

ENUM = {
    "Shader": 1,
    "Logical": 0,
    "GLSL450": 1,
    "Vertex": 0,
    "Fragment": 4,
    "OriginUpperLeft": 7,
    "BuiltIn": 11,
    "VertexIndex": 42,
    "Position": 0,
    "Location": 30,
    "Input": 1,
    "Output": 3,
    "None": 0,
}

STR_OPS = {"OpName", "OpEntryPoint"}


def str_words(s: str) -> list[int]:
    raw = s.encode("utf-8") + b"\x00"
    pad = (4 - (len(raw) % 4)) % 4
    raw += b"\x00" * pad
    words = list(struct.unpack("<" + "I" * (len(raw) // 4), raw))
    return words


def assemble(text: str) -> bytes:
    ids: dict[str, int] = {}
    next_id = 1

    def intern(name: str) -> int:
        nonlocal next_id
        if name not in ids:
            ids[name] = next_id
            next_id += 1
        return ids[name]

    lines: list[tuple[str | None, str, list[str]]] = []
    for raw in text.splitlines():
        line = raw.split(";", 1)[0].strip()
        if not line:
            continue
        result = None
        if "=" in line and not line.startswith("Op"):
            result, line = [p.strip() for p in line.split("=", 1)]
            intern(result.lstrip("%"))
        op, *rest = line.split()
        # keep quoted strings as one token
        tokens = re.findall(r'"[^"]*"|%[A-Za-z0-9_]+|-?\d+\.\d+|-?\d+|[A-Za-z0-9_]+', line)
        op = tokens[0]
        args = tokens[1:]
        if result:
            intern(result.lstrip("%"))
        for a in args:
            if a.startswith("%"):
                intern(a[1:])
        lines.append((result, op, args))

    bound = max(ids.values(), default=0) + 1
    words = [0x07230203, 0x00010500, 0, bound, 0]
    float_types = {result.lstrip("%") for result, op, _ in lines if op == "OpTypeFloat" and result}

    def tok(a: str) -> list[int]:
        if a.startswith("%"):
            return [ids[a[1:]]]
        if a.startswith('"') and a.endswith('"'):
            return str_words(a[1:-1])
        if a in ENUM:
            return [ENUM[a]]
        if re.fullmatch(r"-?\d+\.\d+", a):
            return [struct.unpack("<I", struct.pack("<f", float(a)))[0]]
        if re.fullmatch(r"-?\d+", a):
            return [int(a) & 0xFFFFFFFF]
        raise ValueError(f"bad token {a}")

    for result, op, args in lines:
        opcode = OP[op]
        payload: list[int] = []
        if result:
            # result-type is first arg for typed ops; handled below via args
            pass
        # Typed instructions: %id = OpXxx %type ...
        if result:
            # For Type* the result id is the type itself; operands follow opcode
            if op.startswith("OpType"):
                payload.append(ids[result.lstrip("%")])
                for a in args:
                    payload.extend(tok(a))
            elif op in ("OpFunction", "OpLoad", "OpConstant", "OpConstantComposite", "OpVariable", "OpCompositeExtract", "OpCompositeConstruct", "OpLabel"):
                if op == "OpLabel":
                    payload.append(ids[result.lstrip("%")])
                else:
                    # result type is args[0], result id is result
                    payload.extend(tok(args[0]))
                    payload.append(ids[result.lstrip("%")])
                    type_name = args[0].lstrip("%") if args and args[0].startswith("%") else ""
                    for a in args[1:]:
                        if (
                            op == "OpConstant"
                            and type_name in float_types
                            and re.fullmatch(r"-?\d+(\.\d+)?", a)
                        ):
                            payload.append(struct.unpack("<I", struct.pack("<f", float(a)))[0])
                        else:
                            payload.extend(tok(a))
            else:
                raise ValueError(op)
        else:
            for a in args:
                payload.extend(tok(a))
        inst = [(len(payload) + 1) << 16 | opcode] + payload
        words.extend(inst)

    return struct.pack("<" + "I" * len(words), *words)


def main() -> None:
    src = Path(sys.argv[1])
    dst = Path(sys.argv[2])
    dst.write_bytes(assemble(src.read_text()))
    print(f"wrote {dst} ({dst.stat().st_size} bytes)")


if __name__ == "__main__":
    main()
