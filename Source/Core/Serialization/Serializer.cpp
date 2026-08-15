#include "Core/Serialization/Serializer.h"

#include <string>

namespace harpia::serial {
namespace {

using reflect::FieldInfo;
using reflect::FieldKind;
using reflect::TypeInfo;

[[nodiscard]] std::size_t scalarSize(FieldKind kind) noexcept
{
    switch (kind) {
        case FieldKind::Bool:   return 1;
        case FieldKind::Int8:
        case FieldKind::UInt8:  return 1;
        case FieldKind::Int16:
        case FieldKind::UInt16: return 2;
        case FieldKind::Int32:
        case FieldKind::UInt32:
        case FieldKind::Float:  return 4;
        case FieldKind::Int64:
        case FieldKind::UInt64:
        case FieldKind::Double: return 8;
        default:                return 0;
    }
}

void writeValue(FieldKind        kind,
                std::size_t      valueSize,
                const TypeInfo*  structType,
                const void*      value,
                ByteWriter&      writer);

void writeStruct(const TypeInfo& type, const void* object, ByteWriter& writer);

void writeValue(FieldKind        kind,
                std::size_t      valueSize,
                const TypeInfo*  structType,
                const void*      value,
                ByteWriter&      writer)
{
    switch (kind) {
        case FieldKind::String: {
            writer.writeString(*static_cast<const std::string*>(value));
            break;
        }
        case FieldKind::Struct: {
            if (structType != nullptr) {
                writeStruct(*structType, value, writer);
            }
            break;
        }
        case FieldKind::Enum: {
            // Enums travel as their storage width; the underlying size is what
            // the field reported.
            writer.writeBytes(value, valueSize);
            break;
        }
        default: {
            writer.writeBytes(value, scalarSize(kind));
            break;
        }
    }
}

void writeField(const FieldInfo& field, const void* object, ByteWriter& writer)
{
    const void* value = field.constGet(object);

    writer.writeString(field.name);
    writer.writeRaw(static_cast<std::uint8_t>(field.kind));

    const std::size_t lengthAt = writer.beginPatchableLength();

    if (field.kind == FieldKind::Vector) {
        const std::size_t count = field.vectorOps.size(value);
        writer.writeRaw(static_cast<std::uint32_t>(count));
        for (std::size_t i = 0; i < count; ++i) {
            const void* element = field.vectorOps.constAt(value, i);
            writeValue(field.elementKind, field.elementSize, field.structType, element, writer);
        }
    } else {
        writeValue(field.kind, field.size, field.structType, value, writer);
    }

    writer.endPatchableLength(lengthAt);
}

void writeStruct(const TypeInfo& type, const void* object, ByteWriter& writer)
{
    writer.writeString(type.name);
    writer.writeRaw(type.version);
    writer.writeRaw(static_cast<std::uint32_t>(type.fields.size()));

    for (const FieldInfo& field : type.fields) {
        writeField(field, object, writer);
    }
}

// --- reading ---------------------------------------------------------------

[[nodiscard]] bool readValue(FieldKind       kind,
                             std::size_t     valueSize,
                             const TypeInfo* structType,
                             void*           out,
                             ByteReader&     reader,
                             LoadResult&     result);

// outStoredVersion is only passed by the root call. Nested structs carry their
// own versions and must not overwrite the one the caller asked about.
[[nodiscard]] LoadStatus readStruct(const TypeInfo& type,
                                    void*           object,
                                    ByteReader&     reader,
                                    LoadResult&     result,
                                    std::uint32_t*  outStoredVersion = nullptr);

bool readValue(FieldKind       kind,
               std::size_t     valueSize,
               const TypeInfo* structType,
               void*           out,
               ByteReader&     reader,
               LoadResult&     result)
{
    switch (kind) {
        case FieldKind::String:
            return reader.readString(*static_cast<std::string*>(out));

        case FieldKind::Struct: {
            if (structType == nullptr) {
                return false;
            }
            return readStruct(*structType, out, reader, result) == LoadStatus::Ok;
        }

        case FieldKind::Enum:
            return reader.readBytes(out, valueSize);

        default:
            return reader.readBytes(out, scalarSize(kind));
    }
}

LoadStatus readStruct(const TypeInfo& type,
                      void*           object,
                      ByteReader&     reader,
                      LoadResult&     result,
                      std::uint32_t*  outStoredVersion)
{
    std::string storedName;
    if (!reader.readString(storedName)) {
        return LoadStatus::Truncated;
    }
    if (storedName != type.name) {
        return LoadStatus::TypeMismatch;
    }

    std::uint32_t storedVersion = 0;
    if (!reader.readRaw(storedVersion)) {
        return LoadStatus::Truncated;
    }
    if (outStoredVersion != nullptr) {
        *outStoredVersion = storedVersion;
    }

    std::uint32_t fieldCount = 0;
    if (!reader.readRaw(fieldCount)) {
        return LoadStatus::Truncated;
    }

    std::vector<bool> seen(type.fields.size(), false);

    for (std::uint32_t i = 0; i < fieldCount; ++i) {
        std::string fieldName;
        if (!reader.readString(fieldName)) {
            return LoadStatus::Truncated;
        }

        std::uint8_t rawKind = 0;
        if (!reader.readRaw(rawKind)) {
            return LoadStatus::Truncated;
        }

        std::uint32_t payloadLength = 0;
        if (!reader.readRaw(payloadLength)) {
            return LoadStatus::Truncated;
        }

        const std::size_t payloadEnd = reader.position() + payloadLength;
        const FieldInfo*  field      = type.findField(fieldName);

        // Length prefix earns its bytes here: an unknown or retyped field is
        // stepped over without corrupting everything after it.
        const bool usable = field != nullptr
                         && static_cast<std::uint8_t>(field->kind) == rawKind;

        if (!usable) {
            ++result.skippedFields;
            if (!reader.skip(payloadLength)) {
                return LoadStatus::Truncated;
            }
            continue;
        }

        const auto fieldIndex = static_cast<std::size_t>(field - type.fields.data());
        seen[fieldIndex] = true;

        void* target = field->get(object);

        if (field->kind == FieldKind::Vector) {
            std::uint32_t count = 0;
            if (!reader.readRaw(count)) {
                return LoadStatus::Truncated;
            }
            field->vectorOps.resize(target, count);
            for (std::uint32_t element = 0; element < count; ++element) {
                void* slot = field->vectorOps.at(target, element);
                if (!readValue(field->elementKind, field->elementSize,
                               field->structType, slot, reader, result)) {
                    return LoadStatus::Truncated;
                }
            }
        } else if (!readValue(field->kind, field->size, field->structType,
                              target, reader, result)) {
            return LoadStatus::Truncated;
        }

        // Trust the length prefix over the walk: a nested type that changed
        // shape must not desynchronise the outer object.
        if (reader.position() != payloadEnd) {
            if (reader.position() > payloadEnd) {
                return LoadStatus::Truncated;
            }
            if (!reader.skip(payloadEnd - reader.position())) {
                return LoadStatus::Truncated;
            }
        }
    }

    for (std::size_t i = 0; i < seen.size(); ++i) {
        if (!seen[i]) {
            ++result.defaultedFields;
        }
    }

    if (storedVersion != type.version && type.migrate != nullptr) {
        type.migrate(object, storedVersion);
        result.migrated = true;
    }

    return LoadStatus::Ok;
}

} // namespace

const char* toString(LoadStatus status) noexcept
{
    switch (status) {
        case LoadStatus::Ok:                return "Ok";
        case LoadStatus::BadMagic:          return "BadMagic";
        case LoadStatus::UnsupportedFormat: return "UnsupportedFormat";
        case LoadStatus::UnknownType:       return "UnknownType";
        case LoadStatus::Truncated:         return "Truncated";
        case LoadStatus::TypeMismatch:      return "TypeMismatch";
    }
    return "Unknown";
}

void save(const reflect::TypeInfo& type, const void* object, ByteWriter& writer)
{
    writer.writeRaw(kMagic);
    writer.writeRaw(kFormatVersion);
    writeStruct(type, object, writer);
}

std::vector<std::uint8_t> saveToBytes(const reflect::TypeInfo& type, const void* object)
{
    ByteWriter writer;
    save(type, object, writer);
    return writer.take();
}

LoadResult load(const reflect::TypeInfo& type, void* object, ByteReader& reader)
{
    LoadResult result;

    std::uint32_t magic = 0;
    if (!reader.readRaw(magic)) {
        result.status = LoadStatus::Truncated;
        return result;
    }
    if (magic != kMagic) {
        result.status = LoadStatus::BadMagic;
        return result;
    }

    std::uint32_t format = 0;
    if (!reader.readRaw(format)) {
        result.status = LoadStatus::Truncated;
        return result;
    }
    if (format > kFormatVersion) {
        result.status = LoadStatus::UnsupportedFormat;
        return result;
    }

    result.status = readStruct(type, object, reader, result, &result.sourceVersion);
    return result;
}

LoadResult loadFromBytes(const reflect::TypeInfo&      type,
                         void*                         object,
                         std::span<const std::uint8_t> bytes)
{
    ByteReader reader(bytes);
    return load(type, object, reader);
}

} // namespace harpia::serial
