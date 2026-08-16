#include "Navigation/BehaviorTree.h"

namespace harpia::bt {

Sequence& Sequence::add(std::unique_ptr<Node> child)
{
    children_.push_back(std::move(child));
    return *this;
}

Status Sequence::tick(Blackboard& board, float dt)
{
    while (current_ < children_.size()) {
        const Status status = children_[current_]->tick(board, dt);
        if (status == Status::Running) {
            return Status::Running;
        }
        if (status == Status::Failure) {
            reset();
            return Status::Failure;
        }
        ++current_;
    }
    reset();
    return Status::Success;
}

void Sequence::reset()
{
    current_ = 0;
    for (auto& child : children_) {
        child->reset();
    }
}

Selector& Selector::add(std::unique_ptr<Node> child)
{
    children_.push_back(std::move(child));
    return *this;
}

Status Selector::tick(Blackboard& board, float dt)
{
    while (current_ < children_.size()) {
        const Status status = children_[current_]->tick(board, dt);
        if (status == Status::Running) {
            return Status::Running;
        }
        if (status == Status::Success) {
            reset();
            return Status::Success;
        }
        ++current_;
    }
    reset();
    return Status::Failure;
}

void Selector::reset()
{
    current_ = 0;
    for (auto& child : children_) {
        child->reset();
    }
}

Inverter::Inverter(std::unique_ptr<Node> child) : child_(std::move(child)) {}

Status Inverter::tick(Blackboard& board, float dt)
{
    if (child_ == nullptr) {
        return Status::Failure;
    }
    const Status status = child_->tick(board, dt);
    if (status == Status::Running) {
        return Status::Running;
    }
    return status == Status::Success ? Status::Failure : Status::Success;
}

void Inverter::reset()
{
    if (child_ != nullptr) {
        child_->reset();
    }
}

Action::Action(Fn fn) : fn_(std::move(fn)) {}

Status Action::tick(Blackboard& board, float dt)
{
    if (!fn_) {
        return Status::Failure;
    }
    return fn_(board, dt);
}

Condition::Condition(Fn fn) : fn_(std::move(fn)) {}

Status Condition::tick(Blackboard& board, float)
{
    if (!fn_) {
        return Status::Failure;
    }
    return fn_(board) ? Status::Success : Status::Failure;
}

} // namespace harpia::bt
