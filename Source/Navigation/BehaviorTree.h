// Harpia Engine — behavior trees
//
// A handful of composite nodes, not a visual editor. Sequence and Selector
// are the whole language; Action/Condition are the leaves the game fills in.
// State lives on the node so a Running child is resumed, not restarted.
#pragma once

#include "Core/Math/Math.h"

#include <cstdint>
#include <functional>
#include <memory>
#include <string>
#include <string_view>
#include <unordered_map>
#include <vector>

namespace harpia::bt {

enum class Status : std::uint8_t {
    Success = 0,
    Failure = 1,
    Running = 2,
};

class Blackboard {
public:
    void setFloat(std::string name, float value) { floats_[std::move(name)] = value; }
    void setVec3(std::string name, Vec3 value) { vecs_[std::move(name)] = value; }
    void setInt(std::string name, std::int32_t value) { ints_[std::move(name)] = value; }

    [[nodiscard]] float getFloat(std::string_view name, float fallback = 0.0f) const
    {
        const auto it = floats_.find(std::string(name));
        return it == floats_.end() ? fallback : it->second;
    }

    [[nodiscard]] Vec3 getVec3(std::string_view name, Vec3 fallback = {}) const
    {
        const auto it = vecs_.find(std::string(name));
        return it == vecs_.end() ? fallback : it->second;
    }

    [[nodiscard]] std::int32_t getInt(std::string_view name, std::int32_t fallback = 0) const
    {
        const auto it = ints_.find(std::string(name));
        return it == ints_.end() ? fallback : it->second;
    }

    void* user = nullptr;

private:
    std::unordered_map<std::string, float>        floats_;
    std::unordered_map<std::string, Vec3>         vecs_;
    std::unordered_map<std::string, std::int32_t> ints_;
};

class Node {
public:
    virtual ~Node() = default;
    virtual Status tick(Blackboard& board, float dt) = 0;
    virtual void   reset() {}
};

class Sequence final : public Node {
public:
    Sequence& add(std::unique_ptr<Node> child);
    Status    tick(Blackboard& board, float dt) override;
    void      reset() override;

private:
    std::vector<std::unique_ptr<Node>> children_;
    std::size_t                        current_ = 0;
};

class Selector final : public Node {
public:
    Selector& add(std::unique_ptr<Node> child);
    Status    tick(Blackboard& board, float dt) override;
    void      reset() override;

private:
    std::vector<std::unique_ptr<Node>> children_;
    std::size_t                        current_ = 0;
};

class Inverter final : public Node {
public:
    explicit Inverter(std::unique_ptr<Node> child);
    Status tick(Blackboard& board, float dt) override;
    void   reset() override;

private:
    std::unique_ptr<Node> child_;
};

class Action final : public Node {
public:
    using Fn = std::function<Status(Blackboard&, float)>;
    explicit Action(Fn fn);
    Status tick(Blackboard& board, float dt) override;

private:
    Fn fn_;
};

class Condition final : public Node {
public:
    using Fn = std::function<bool(const Blackboard&)>;
    explicit Condition(Fn fn);
    Status tick(Blackboard& board, float dt) override;

private:
    Fn fn_;
};

} // namespace harpia::bt
