#include "Core/Reflection/TypeRegistry.h"

#include <algorithm>
#include <cassert>
#include <memory>
#include <mutex>
#include <unordered_map>

namespace harpia::reflect {
namespace {

struct Storage {
    std::mutex mutex;
    // unique_ptr keeps TypeInfo addresses stable as the map grows; consumers
    // hold raw pointers for the process lifetime.
    std::unordered_map<std::string, std::unique_ptr<TypeInfo>> byName;
    std::vector<const TypeInfo*>                               ordered;
};

Storage& storage()
{
    static Storage instance;
    return instance;
}

} // namespace

const char* toString(FieldKind kind) noexcept
{
    switch (kind) {
        case FieldKind::Bool:   return "bool";
        case FieldKind::Int8:   return "int8";
        case FieldKind::Int16:  return "int16";
        case FieldKind::Int32:  return "int32";
        case FieldKind::Int64:  return "int64";
        case FieldKind::UInt8:  return "uint8";
        case FieldKind::UInt16: return "uint16";
        case FieldKind::UInt32: return "uint32";
        case FieldKind::UInt64: return "uint64";
        case FieldKind::Float:  return "float";
        case FieldKind::Double: return "double";
        case FieldKind::String: return "string";
        case FieldKind::Enum:   return "enum";
        case FieldKind::Struct: return "struct";
        case FieldKind::Vector: return "vector";
    }
    return "unknown";
}

bool isScalar(FieldKind kind) noexcept
{
    switch (kind) {
        case FieldKind::String:
        case FieldKind::Struct:
        case FieldKind::Vector:
            return false;
        default:
            return true;
    }
}

const FieldInfo* TypeInfo::findField(std::string_view fieldName) const noexcept
{
    const auto it = std::find_if(fields.begin(), fields.end(),
                                 [fieldName](const FieldInfo& field) {
                                     return fieldName == field.name;
                                 });
    return it != fields.end() ? &*it : nullptr;
}

const TypeInfo* TypeRegistry::add(TypeInfo&& info)
{
    Storage& s = storage();
    std::lock_guard<std::mutex> lock(s.mutex);

    const auto existing = s.byName.find(info.name);
    if (existing != s.byName.end()) {
        // Two definitions under one name means someone reflected the same type
        // twice or reused a name; both corrupt deserialisation silently.
        assert(existing->second->version == info.version
               && existing->second->fields.size() == info.fields.size()
               && "type registered twice with a different definition");
        return existing->second.get();
    }

    auto stored = std::make_unique<TypeInfo>(std::move(info));
    const TypeInfo* raw = stored.get();

    s.byName.emplace(raw->name, std::move(stored));
    s.ordered.push_back(raw);
    return raw;
}

const TypeInfo* TypeRegistry::find(std::string_view name) noexcept
{
    Storage& s = storage();
    std::lock_guard<std::mutex> lock(s.mutex);

    const auto it = s.byName.find(std::string{name});
    return it != s.byName.end() ? it->second.get() : nullptr;
}

std::vector<const TypeInfo*> TypeRegistry::all()
{
    Storage& s = storage();
    std::lock_guard<std::mutex> lock(s.mutex);
    return s.ordered;
}

std::size_t TypeRegistry::count() noexcept
{
    Storage& s = storage();
    std::lock_guard<std::mutex> lock(s.mutex);
    return s.ordered.size();
}

} // namespace harpia::reflect
