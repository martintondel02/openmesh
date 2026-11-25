// SPDX-License-Identifier: AGPL-3.0-or-later
// Tiny, allocation-free CBOR writer/reader for the OMP/1 payload schemas.
//
// OMP/1 uses CBOR as encoded by Rust's `serde` + `ciborium` (PROTOCOL.md §5.3). The writer
// below emits the same canonical form: definite-length maps, fields in schema declaration
// order, minimal-width integers, `None` fields omitted. The reader tolerates unknown keys and
// both definite and indefinite lengths, as required by PROTOCOL.md §16.
//
// Hand-rolled on purpose: it removes a dependency from the firmware build and is small enough
// to audit (see firmware/README.md "CBOR" for the rationale).
#ifndef OPENMESH_OM_CBOR_H
#define OPENMESH_OM_CBOR_H

#include <cstddef>
#include <cstdint>

namespace om {

class CborWriter {
public:
    CborWriter(uint8_t* buffer, size_t capacity);

    void reset(uint8_t* buffer, size_t capacity);
    bool ok() const { return ok_; }
    size_t size() const { return len_; }
    const uint8_t* data() const { return buf_; }

    bool mapHeader(size_t pair_count);
    bool arrayHeader(size_t item_count);
    bool key(const char* k);
    bool text(const char* s);
    bool text(const char* s, size_t n);
    bool bytes(const uint8_t* data, size_t n);
    bool uint(uint64_t v);
    bool int64Value(int64_t v);
    bool boolean(bool v);
    bool nullValue();
    // Appends an already-encoded CBOR item verbatim (used to embed nested structs without a
    // second encoder pass).
    bool rawItem(const uint8_t* data, size_t len);

private:
    bool head(uint8_t major, uint64_t value);
    bool raw(const void* data, size_t n);

    uint8_t* buf_;
    size_t cap_;
    size_t len_ = 0;
    bool ok_ = true;
};

class CborReader {
public:
    CborReader(const uint8_t* data, size_t len);

    bool ok() const { return ok_; }
    bool eof() const { return pos_ >= len_; }
    size_t pos() const { return pos_; }
    const uint8_t* cursor() const { return data_ + pos_; }
    size_t remaining() const { return len_ - pos_; }
    const uint8_t* base() const { return data_; }

    bool mapHeader(size_t& count);
    bool arrayHeader(size_t& count);
    // Reads a text string key and compares it with `expected`. Mismatched keys are consumed
    // and return false, allowing `skipValue()` to follow for forward compatibility.
    bool key(const char* expected);
    // Reads a map key as a text span without consuming the corresponding value.
    bool readKeySpan(const uint8_t*& text, size_t& len);
    bool readText(const uint8_t*& text, size_t& len);
    bool readBytes(const uint8_t*& data, size_t& len);
    bool readUint(uint64_t& out);
    bool readInt(int64_t& out);
    bool readBool(bool& out);
    // Reads the next value as a uint8/16 (accepting wider encodings when they fit).
    bool readU8(uint8_t& out);
    bool readU16(uint16_t& out);
    bool readU32(uint32_t& out);

    // Skips exactly one serialised item (map keys or values).
    bool skipValue();
    // Like skipValue(), but records the byte offset/length of the skipped item relative to
    // base(). Used to verify signatures over "the CBOR bytes as transmitted".
    bool skipValueSpan(size_t& offset, size_t& length);

private:
    bool readHead(uint8_t& major, uint64_t& value);
    bool readLength(uint8_t major, size_t& len);

    const uint8_t* data_;
    size_t len_;
    size_t pos_ = 0;
    bool ok_ = true;
};

}  // namespace om

#endif  // OPENMESH_OM_CBOR_H
