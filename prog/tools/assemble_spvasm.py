#!/usr/bin/env python3
"""Minimal SPIR-V assembler for Harpia .spvasm files. Prefer spirv-as / DXC when present."""
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
    "OpExtension": 10,
    "OpName": 5,
    "OpDecorate": 71,
    "OpMemberDecorate": 72,
    "OpTypeVoid": 19,
    "OpTypeBool": 20,
    "OpTypeFunction": 33,
    "OpTypeInt": 21,
    "OpTypeFloat": 22,
    "OpTypeVector": 23,
    "OpTypeImage": 25,
    "OpTypeSampler": 26,
    "OpTypeSampledImage": 27,
    "OpTypeArray": 28,
    "OpTypeRuntimeArray": 29,
    "OpTypeStruct": 30,
    "OpTypePointer": 32,
    "OpVariable": 59,
    "OpConstant": 43,
    "OpConstantComposite": 44,
    "OpFunction": 54,
    "OpFunctionEnd": 56,
    "OpLabel": 248,
    "OpLoad": 61,
    "OpStore": 62,
    "OpAccessChain": 65,
    "OpSampledImage": 86,
    "OpImageSampleImplicitLod": 87,
    "OpImageWrite": 99,
    "OpReturn": 253,
    "OpSelectionMerge": 247,
    "OpSwitch": 251,
    "OpBranch": 249,
    "OpCompositeExtract": 81,
    "OpCompositeConstruct": 80,
    "OpUDiv": 134,
    "OpSDiv": 135,
    "OpUMod": 137,
    "OpSRem": 138,
    "OpSMod": 139,
    "OpIAdd": 128,
    "OpISub": 130,
    "OpIMul": 132,
    "OpIEqual": 170,
    "OpINotEqual": 171,
    "OpUGreaterThan": 172,
    "OpUGreaterThanEqual": 174,
    "OpULessThan": 176,
    "OpULessThanEqual": 178,
    "OpSelect": 169,
    "OpFMul": 133,
    "OpFAdd": 129,
    "OpFSub": 131,
    "OpFDiv": 136,
    "OpFNegate": 127,
    "OpDot": 148,
    "OpVectorTimesScalar": 142,
    "OpVectorTimesMatrix": 144,
    "OpMatrixTimesVector": 145,
    "OpMatrixTimesMatrix": 146,
    "OpExtInstImport": 11,
    "OpExtInst": 12,
    "OpImageSampleExplicitLod": 88,
    "OpImageSampleDrefImplicitLod": 89,
    "OpImageSampleDrefExplicitLod": 90,
    "OpImageSampleProjImplicitLod": 91,
    "OpImageSampleProjExplicitLod": 92,
    "OpImageFetch": 95,
    "OpImageGather": 96,
    "OpImageDrefGather": 97,
    "OpVectorExtractDynamic": 77,
    "OpVectorInsertDynamic": 78,
    # SPIR-V 3.42.15: the Ord/Unord pairs are interleaved. 181 is OpFUnordEqual,
    # NOT OpFOrdLessThanEqual (that mistake turned `a <= b` into `a == b`).
    "OpFOrdEqual": 180,
    "OpFUnordEqual": 181,
    "OpFOrdNotEqual": 182,
    "OpFUnordNotEqual": 183,
    "OpFOrdLessThan": 184,
    "OpFUnordLessThan": 185,
    "OpFOrdGreaterThan": 186,
    "OpFUnordGreaterThan": 187,
    "OpFOrdLessThanEqual": 188,
    "OpFUnordLessThanEqual": 189,
    "OpFOrdGreaterThanEqual": 190,
    "OpFUnordGreaterThanEqual": 191,
    "OpTypeMatrix": 24,
    "OpFunctionParameter": 55,
    "OpFunctionCall": 57,
    "OpReturnValue": 254,
    "OpVectorShuffle": 79,
    "OpCompositeInsert": 82,
    "OpConvertFToU": 109,
    "OpConvertUToF": 112,
    "OpConvertFToS": 110,
    "OpConvertSToF": 111,
    "OpBitcast": 124,
    "OpLogicalOr": 166,
    "OpLogicalAnd": 167,
    "OpLogicalNot": 168,
    "OpBranchConditional": 250,
    "OpPhi": 245,
    "OpKill": 252,
    "OpLoopMerge": 246,
    "OpImageRead": 98,
}

ENUM = {
    "Shader": 1,
    "Logical": 0,
    "GLSL450": 1,
    "Vertex": 0,
    "Fragment": 4,
    "GLCompute": 5,
    "OriginUpperLeft": 7,
    "LocalSize": 17,
    "BuiltIn": 11,
    "VertexIndex": 42,
    "Position": 0,
    "GlobalInvocationId": 28,
    "Location": 30,
    "Binding": 33,
    "DescriptorSet": 34,
    "Offset": 35,
    "Block": 2,
    "NonUniform": 5300,
    "Input": 1,
    "Output": 3,
    "UniformConstant": 0,
    "Uniform": 2,
    "Function": 7,
    "None": 0,
    "1D": 0,
    "2D": 1,
    "3D": 2,
    "Unknown": 0,
    "Rgba32f": 1,
    "Rgba16f": 2,
    "R32f": 3,
    "Rgba8": 4,
    "RuntimeDescriptorArray": 5302,
    "SampledImageArrayNonUniformIndexing": 5307,
    "ShaderNonUniform": 5301,
    "SampledImageArrayDynamicIndexing": 29,
    "PushConstant": 9,
    "ColMajor": 5,
    "MatrixStride": 7,
    "Lod": 2,
    "ArrayStride": 6,
    "FragCoord": 15,
    "Acos": 17,
    "Atan2": 25,
    "FAbs": 4,
    "FSign": 6,
    "Floor": 8,
    "Fract": 10,
    "Sin": 13,
    "Cos": 14,
    "Pow": 26,
    "Exp": 27,
    "Log": 28,
    "Exp2": 29,
    "Log2": 30,
    "Sqrt": 31,
    "InverseSqrt": 32,
    "FMin": 37,
    "FMax": 40,
    "FClamp": 43,
    "FMix": 46,
    "Length": 66,
    "Distance": 67,
    "Cross": 68,
    "Normalize": 69,
    "Reflect": 71,
    "Ceil": 9,
    "Tan": 15,
    "Step": 48,
    "SmoothStep": 49,
}


def str_words(s: str) -> list[int]:
    raw = s.encode("utf-8") + b"\x00"
    pad = (4 - (len(raw) % 4)) % 4
    raw += b"\x00" * pad
    return list(struct.unpack("<" + "I" * (len(raw) // 4), raw))


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
        tokens = re.findall(r'"[^"]*"|%[A-Za-z0-9_]+|-?\d+\.\d+|[A-Za-z0-9_]+|-?\d+', line)
        op = tokens[0]
        args = tokens[1:]
        if result:
            intern(result.lstrip("%"))
        for a in args:
            if a.startswith("%"):
                intern(a[1:])
        lines.append((result, op, args))

    # Cheap module checks. spirv-val catches these too, but its message is
    # sometimes empty, and by then you are debugging a GPU instead of a text file.
    defined: set[str] = set()
    for result, op, _ in lines:
        if not result:
            continue
        name = result.lstrip("%")
        if name in defined:
            raise ValueError(f"id %{name} is defined twice (second time by {op})")
        defined.add(name)
    for result, op, args in lines:
        for a in args:
            if a.startswith("%") and a[1:] not in defined:
                raise ValueError(f"{op} uses %{a[1:]}, which nothing defines")

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
            if op.startswith("OpType"):
                payload.append(ids[result.lstrip("%")])
                for a in args:
                    payload.extend(tok(a))
            elif op in ("OpLabel", "OpExtInstImport"):
                payload.append(ids[result.lstrip("%")])
                for a in args:
                    payload.extend(tok(a))
            else:
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
