// SPDX-License-Identifier: AGPL-3.0-or-later
// Minimal recursive-descent JSON parser for golden-vector replay in host tests.
// Supports the JSON subset emitted by std/serde_json: objects, arrays, strings with escapes,
// numbers (kept as raw text so u64 ids do not lose precision), true/false/null.
#include "json_lite.h"

#include <cctype>
#include <cstdlib>
#include <cstring>

namespace json {

const Value* Value::get(const char* key) const {
    return get(std::string(key));
}

const Value* Value::get(const std::string& key) const {
    if (type != Type::Object) return nullptr;
    for (const auto& kv : object) {
        if (kv.first == key) return &kv.second;
    }
    return nullptr;
}

const Value* Value::getAny(std::initializer_list<const char*> keys) const {
    for (const char* key : keys) {
        const Value* v = get(key);
        if (v != nullptr) return v;
    }
    return nullptr;
}

uint64_t Value::asU64() const {
    if (isNumber()) {
        if (!number_raw.empty()) return (uint64_t)std::strtoull(number_raw.c_str(), nullptr, 10);
        return (uint64_t)number;
    }
    if (isString()) return (uint64_t)std::strtoull(string.c_str(), nullptr, 10);
    if (isBool()) return boolean ? 1 : 0;
    return 0;
}

int64_t Value::asI64() const {
    if (isNumber()) {
        if (!number_raw.empty()) return (int64_t)std::strtoll(number_raw.c_str(), nullptr, 10);
        return (int64_t)number;
    }
    if (isString()) return (int64_t)std::strtoll(string.c_str(), nullptr, 10);
    return 0;
}

namespace {

struct Parser {
    const std::string& text;
    size_t pos = 0;
    std::string error;

    explicit Parser(const std::string& t) : text(t) {}

    void skipWs() {
        while (pos < text.size()) {
            const char c = text[pos];
            if (c == ' ' || c == '\t' || c == '\n' || c == '\r') {
                ++pos;
            } else if (c == '/' && pos + 1 < text.size() && text[pos + 1] == '/') {
                while (pos < text.size() && text[pos] != '\n') ++pos;
            } else {
                break;
            }
        }
    }

    bool fail(const char* message) {
        if (error.empty()) error = std::string(message) + " at offset " + std::to_string(pos);
        return false;
    }

    bool parseValue(Value& out) {
        skipWs();
        if (pos >= text.size()) return fail("unexpected end of input");
        const char c = text[pos];
        if (c == '{') return parseObject(out);
        if (c == '[') return parseArray(out);
        if (c == '"') {
            out.type = Type::String;
            return parseString(out.string);
        }
        if (c == 't') {
            if (text.compare(pos, 4, "true") != 0) return fail("bad literal");
            pos += 4;
            out.type = Type::Bool;
            out.boolean = true;
            return true;
        }
        if (c == 'f') {
            if (text.compare(pos, 5, "false") != 0) return fail("bad literal");
            pos += 5;
            out.type = Type::Bool;
            out.boolean = false;
            return true;
        }
        if (c == 'n') {
            if (text.compare(pos, 4, "null") != 0) return fail("bad literal");
            pos += 4;
            out.type = Type::Null;
            return true;
        }
        if (c == '-' || (c >= '0' && c <= '9')) return parseNumber(out);
        return fail("unexpected character");
    }

    bool parseNumber(Value& out) {
        const size_t start = pos;
        if (pos < text.size() && text[pos] == '-') ++pos;
        while (pos < text.size() && std::isdigit((unsigned char)text[pos])) ++pos;
        if (pos < text.size() && text[pos] == '.') {
            ++pos;
            while (pos < text.size() && std::isdigit((unsigned char)text[pos])) ++pos;
        }
        if (pos < text.size() && (text[pos] == 'e' || text[pos] == 'E')) {
            ++pos;
            if (pos < text.size() && (text[pos] == '+' || text[pos] == '-')) ++pos;
            while (pos < text.size() && std::isdigit((unsigned char)text[pos])) ++pos;
        }
        if (pos == start) return fail("bad number");
        out.type = Type::Number;
        out.number_raw = text.substr(start, pos - start);
        out.number = std::strtod(out.number_raw.c_str(), nullptr);
        return true;
    }

    bool parseString(std::string& out) {
        if (pos >= text.size() || text[pos] != '"') return fail("expected string");
        ++pos;
        out.clear();
        while (pos < text.size()) {
            const char c = text[pos++];
            if (c == '"') return true;
            if (c == '\\') {
                if (pos >= text.size()) return fail("bad escape");
                const char e = text[pos++];
                switch (e) {
                    case '"': out.push_back('"'); break;
                    case '\\': out.push_back('\\'); break;
                    case '/': out.push_back('/'); break;
                    case 'b': out.push_back('\b'); break;
                    case 'f': out.push_back('\f'); break;
                    case 'n': out.push_back('\n'); break;
                    case 'r': out.push_back('\r'); break;
                    case 't': out.push_back('\t'); break;
                    case 'u': {
                        if (pos + 4 > text.size()) return fail("bad unicode escape");
                        unsigned code = 0;
                        for (int i = 0; i < 4; ++i) {
                            const char h = text[pos++];
                            code <<= 4;
                            if (h >= '0' && h <= '9') code |= (unsigned)(h - '0');
                            else if (h >= 'a' && h <= 'f') code |= (unsigned)(h - 'a' + 10);
                            else if (h >= 'A' && h <= 'F') code |= (unsigned)(h - 'A' + 10);
                            else return fail("bad hex digit");
                        }
                        // Encode as UTF-8 (surrogate pairs are not needed by our vectors).
                        if (code < 0x80) {
                            out.push_back((char)code);
                        } else if (code < 0x800) {
                            out.push_back((char)(0xC0 | (code >> 6)));
                            out.push_back((char)(0x80 | (code & 0x3F)));
                        } else {
                            out.push_back((char)(0xE0 | (code >> 12)));
                            out.push_back((char)(0x80 | ((code >> 6) & 0x3F)));
                            out.push_back((char)(0x80 | (code & 0x3F)));
                        }
                        break;
                    }
                    default:
                        return fail("unknown escape");
                }
            } else if ((unsigned char)c < 0x20) {
                return fail("control character in string");
            } else {
                out.push_back(c);
            }
        }
        return fail("unterminated string");
    }

    bool parseObject(Value& out) {
        if (text[pos] != '{') return fail("expected object");
        ++pos;
        out.type = Type::Object;
        skipWs();
        if (pos < text.size() && text[pos] == '}') {
            ++pos;
            return true;
        }
        while (true) {
            skipWs();
            std::string key;
            if (!parseString(key)) return false;
            skipWs();
            if (pos >= text.size() || text[pos] != ':') return fail("expected ':'");
            ++pos;
            Value value;
            if (!parseValue(value)) return false;
            out.object.emplace_back(std::move(key), std::move(value));
            skipWs();
            if (pos >= text.size()) return fail("unterminated object");
            if (text[pos] == ',') {
                ++pos;
                continue;
            }
            if (text[pos] == '}') {
                ++pos;
                return true;
            }
            return fail("expected ',' or '}'");
        }
    }

    bool parseArray(Value& out) {
        if (text[pos] != '[') return fail("expected array");
        ++pos;
        out.type = Type::Array;
        skipWs();
        if (pos < text.size() && text[pos] == ']') {
            ++pos;
            return true;
        }
        while (true) {
            Value value;
            if (!parseValue(value)) return false;
            out.array.emplace_back(std::move(value));
            skipWs();
            if (pos >= text.size()) return fail("unterminated array");
            if (text[pos] == ',') {
                ++pos;
                continue;
            }
            if (text[pos] == ']') {
                ++pos;
                return true;
            }
            return fail("expected ',' or ']'");
        }
    }
};

}  // namespace

bool parse(const std::string& text, Value& out, std::string& error) {
    Parser parser(text);
    if (!parser.parseValue(out)) {
        error = parser.error;
        return false;
    }
    parser.skipWs();
    if (parser.pos != text.size()) {
        error = "trailing data at offset " + std::to_string(parser.pos);
        return false;
    }
    return true;
}

}  // namespace json
