// SPDX-License-Identifier: AGPL-3.0-or-later
// Minimal JSON DOM used by the host test suite to replay protocol golden vectors.
// Host-only (uses std::string/std::vector; never compiled into firmware images).
#ifndef OPENMESH_TEST_JSON_LITE_H
#define OPENMESH_TEST_JSON_LITE_H

#include <cstdint>
#include <initializer_list>
#include <string>
#include <utility>
#include <vector>

namespace json {

enum class Type { Null, Bool, Number, String, Array, Object };

struct Value {
    Type type = Type::Null;
    bool boolean = false;
    double number = 0.0;
    std::string number_raw;  // exact text, so 64-bit integers survive
    std::string string;
    std::vector<Value> array;
    std::vector<std::pair<std::string, Value>> object;

    bool isNull() const { return type == Type::Null; }
    bool isBool() const { return type == Type::Bool; }
    bool isNumber() const { return type == Type::Number; }
    bool isString() const { return type == Type::String; }
    bool isArray() const { return type == Type::Array; }
    bool isObject() const { return type == Type::Object; }

    // Object field lookup (nullptr when absent or when this is not an object).
    const Value* get(const char* key) const;
    const Value* get(const std::string& key) const;
    // First present field among a list of candidate names (schema tolerance).
    const Value* getAny(std::initializer_list<const char*> keys) const;

    std::string asString() const { return isString() ? string : std::string(); }
    uint64_t asU64() const;
    int64_t asI64() const;
    double asDouble() const { return isNumber() ? number : 0.0; }
    size_t size() const { return isArray() ? array.size() : object.size(); }
};

bool parse(const std::string& text, Value& out, std::string& error);

}  // namespace json

#endif  // OPENMESH_TEST_JSON_LITE_H
