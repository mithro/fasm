/*
 * Copyright 2017-2022 F4PGA Authors
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 *
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * @file fasm.hpp
 * @brief Header only C++17 RAII wrapper over the fasm C API (fasm.h).
 *
 * See docs/rewrite/DESIGN-capi.md ("C++ wrapper" section) for the design
 * rationale, ownership rules and the exception trampoline used for C
 * callbacks. In short:
 *
 *  - fasm::File owns a `fasm_file *` and frees it in its destructor
 *    (move only, like a `std::unique_ptr`);
 *  - fasm::Line, fasm::SetFeature and fasm::Value are lightweight,
 *    non-owning views (a wrapped pointer) that stay valid exactly as long
 *    as the fasm::File (or, for a streaming callback, the call) they came
 *    from is: the same lifetime rules as the underlying C pointers
 *    (`fasm_line *`, `fasm_set_feature *`) documented in fasm.h;
 *  - fasm::String owns a `fasm_string *`;
 *  - failures are reported as a thrown fasm::Error (a `std::runtime_error`
 *    carrying the status, and for parse errors the line/column), never as
 *    a status code;
 *  - callbacks handed to the C API (`fasm_file_merge_and_sort_ex`,
 *    `fasm_parse_string_cb`) are given as `std::function`s; a small
 *    "trampoline" `extern "C"`-callable function pointer catches any
 *    exception thrown by the C++ callback with `catch (...)`, stores the
 *    FIRST one with `std::current_exception()` (later ones, if the C API
 *    keeps calling the trampoline after that, are discarded) and returns
 *    a value the caller cannot observe (unwinding a C++ exception through
 *    the Rust `extern "C"` boundary is undefined behaviour: see fasm.h
 *    and docs/rewrite/DESIGN-capi.md). Once the C call returns, the
 *    wrapper checks for a stored exception and rethrows it with
 *    `std::rethrow_exception`, so from the caller's point of view the
 *    exception propagates out of `merge_and_sort` / `parse_each` exactly
 *    as if no C boundary were involved. Note the two callback protocols
 *    differ: `parse_each`'s `fasm_line_callback` stops being called as
 *    soon as the trampoline returns `false` (its early-stop protocol), so
 *    only one exception can ever occur there, but `merge_and_sort`'s
 *    `fasm_zero_fn` / `fasm_sort_key_fn` have no such protocol and keep
 *    being invoked for the rest of the model after one has thrown — only
 *    the first exception raised is kept and rethrown.
 *
 *  - fasm::xilinx wraps the `fasm_xilinx_*` functions (Xilinx databases,
 *    FASM -> frames, .frm files, bitstreams) the same way.
 *
 * Thread safety follows fasm.h: a `fasm::File` may be read (including
 * iterated, and have views taken from it) from any number of threads at
 * once; `push_line` needs exclusive access to that `File`. `fasm::String`
 * and `fasm::Error` are immutable and may be used from any thread.
 *
 * This header requires C++ exceptions (and RTTI, for `catch (...)` and
 * `std::current_exception`/`std::rethrow_exception` to work): every
 * failure is a thrown `fasm::Error`, with no non-throwing alternative.
 * It is not usable, and does not attempt to degrade gracefully, when
 * built with `-fno-exceptions` (or the equivalent on other compilers). A
 * caller in that situation uses `fasm.h` directly instead; both are
 * always available side by side (see "Errors" below).
 */

#ifndef FASM_HPP_INCLUDED
#define FASM_HPP_INCLUDED

#include "fasm.h"

#include <cstdint>
#include <exception>
#include <filesystem>
#include <functional>
#include <initializer_list>
#include <iterator>
#include <optional>
#include <stdexcept>
#include <string>
#include <string_view>
#include <type_traits>
#include <utility>
#include <vector>

namespace fasm {

/**
 * @brief Result status of a fallible operation (mirrors `fasm_status`).
 *
 * Kept numerically identical to `fasm_status` so the two convert with a
 * plain `static_cast` (see `to_c(Status)` / `from_c(fasm_status)`).
 */
enum class Status : int {
    Ok = FASM_OK,
    ParseError = FASM_ERR_PARSE,
    Io = FASM_ERR_IO,
    InvalidArg = FASM_ERR_INVALID_ARG,
    Utf8 = FASM_ERR_UTF8,
    Panic = FASM_ERR_PANIC,
    Output = FASM_ERR_OUTPUT,
    Db = FASM_ERR_DB,
    Lookup = FASM_ERR_LOOKUP,
    InconsistentBits = FASM_ERR_INCONSISTENT_BITS,
    Assembler = FASM_ERR_ASSEMBLER,
    Bitstream = FASM_ERR_BITSTREAM,
    Frm = FASM_ERR_FRM,
};

// Each enumerator is defined directly in terms of its fasm_status macro
// above, so these are redundant with that definition today; they are
// still asserted explicitly so that a future edit accidentally breaking
// the 1:1 mapping (e.g. copy-pasting the wrong macro) fails to compile
// instead of silently miscompiling `to_c`/`from_c`.
static_assert(static_cast<int>(Status::Ok) == FASM_OK, "Status::Ok must equal FASM_OK");
static_assert(static_cast<int>(Status::ParseError) == FASM_ERR_PARSE,
              "Status::ParseError must equal FASM_ERR_PARSE");
static_assert(static_cast<int>(Status::Io) == FASM_ERR_IO, "Status::Io must equal FASM_ERR_IO");
static_assert(static_cast<int>(Status::InvalidArg) == FASM_ERR_INVALID_ARG,
              "Status::InvalidArg must equal FASM_ERR_INVALID_ARG");
static_assert(static_cast<int>(Status::Utf8) == FASM_ERR_UTF8,
              "Status::Utf8 must equal FASM_ERR_UTF8");
static_assert(static_cast<int>(Status::Panic) == FASM_ERR_PANIC,
              "Status::Panic must equal FASM_ERR_PANIC");
static_assert(static_cast<int>(Status::Output) == FASM_ERR_OUTPUT,
              "Status::Output must equal FASM_ERR_OUTPUT");
static_assert(static_cast<int>(Status::Db) == FASM_ERR_DB, "Status::Db must equal FASM_ERR_DB");
static_assert(static_cast<int>(Status::Lookup) == FASM_ERR_LOOKUP,
              "Status::Lookup must equal FASM_ERR_LOOKUP");
static_assert(static_cast<int>(Status::InconsistentBits) == FASM_ERR_INCONSISTENT_BITS,
              "Status::InconsistentBits must equal FASM_ERR_INCONSISTENT_BITS");
static_assert(static_cast<int>(Status::Assembler) == FASM_ERR_ASSEMBLER,
              "Status::Assembler must equal FASM_ERR_ASSEMBLER");
static_assert(static_cast<int>(Status::Bitstream) == FASM_ERR_BITSTREAM,
              "Status::Bitstream must equal FASM_ERR_BITSTREAM");
static_assert(static_cast<int>(Status::Frm) == FASM_ERR_FRM, "Status::Frm must equal FASM_ERR_FRM");

/** @brief Converts a `fasm_status` to `Status`. */
constexpr Status from_c(fasm_status status) noexcept { return static_cast<Status>(status); }

/** @brief Converts a `Status` to `fasm_status`. */
constexpr fasm_status to_c(Status status) noexcept { return static_cast<fasm_status>(status); }

/**
 * @brief How a `SetFeature`'s value was written (mirrors `fasm_value_format`).
 *
 * `std::nullopt` (rather than a sixth enumerator) stands for
 * `FASM_VALUE_FORMAT_NONE`: no value was written (an implicit 1), the same
 * choice as `fasm_set_feature_value_format`'s `FASM_VALUE_FORMAT_NONE` /
 * Python's `value_format is None`.
 */
enum class ValueFormat : std::int32_t {
    Plain = FASM_VALUE_FORMAT_PLAIN,
    VerilogDecimal = FASM_VALUE_FORMAT_VERILOG_DECIMAL,
    VerilogHex = FASM_VALUE_FORMAT_VERILOG_HEX,
    VerilogBinary = FASM_VALUE_FORMAT_VERILOG_BINARY,
    VerilogOctal = FASM_VALUE_FORMAT_VERILOG_OCTAL,
};

// See the note above Status's static_asserts: redundant with the direct
// initializers above today, kept as an explicit regression guard.
static_assert(static_cast<std::int32_t>(ValueFormat::Plain) == FASM_VALUE_FORMAT_PLAIN,
              "ValueFormat::Plain must equal FASM_VALUE_FORMAT_PLAIN");
static_assert(static_cast<std::int32_t>(ValueFormat::VerilogDecimal) ==
                  FASM_VALUE_FORMAT_VERILOG_DECIMAL,
              "ValueFormat::VerilogDecimal must equal FASM_VALUE_FORMAT_VERILOG_DECIMAL");
static_assert(static_cast<std::int32_t>(ValueFormat::VerilogHex) == FASM_VALUE_FORMAT_VERILOG_HEX,
              "ValueFormat::VerilogHex must equal FASM_VALUE_FORMAT_VERILOG_HEX");
static_assert(static_cast<std::int32_t>(ValueFormat::VerilogBinary) ==
                  FASM_VALUE_FORMAT_VERILOG_BINARY,
              "ValueFormat::VerilogBinary must equal FASM_VALUE_FORMAT_VERILOG_BINARY");
static_assert(static_cast<std::int32_t>(ValueFormat::VerilogOctal) ==
                  FASM_VALUE_FORMAT_VERILOG_OCTAL,
              "ValueFormat::VerilogOctal must equal FASM_VALUE_FORMAT_VERILOG_OCTAL");
static_assert(static_cast<std::int32_t>(FASM_VALUE_FORMAT_NONE) == -1,
              "FASM_VALUE_FORMAT_NONE must be -1 (std::nullopt stands for it, see from_c/to_c)");

/** @brief Converts a `fasm_value_format`, `FASM_VALUE_FORMAT_NONE` becoming `std::nullopt`. */
constexpr std::optional<ValueFormat> from_c(fasm_value_format format) noexcept {
    if (format == FASM_VALUE_FORMAT_NONE) {
        return std::nullopt;
    }
    return static_cast<ValueFormat>(format);
}

/** @brief Converts to the `int32_t` `fasm_set_feature_spec::value_format` field, `std::nullopt` becoming `FASM_VALUE_FORMAT_NONE`. */
constexpr std::int32_t to_c(std::optional<ValueFormat> format) noexcept {
    return format ? static_cast<std::int32_t>(*format)
                   : static_cast<std::int32_t>(FASM_VALUE_FORMAT_NONE);
}

/**
 * @brief Returns the library version (e.g. `"0.1.0"`).
 *
 * Borrows the static, never freed string `fasm_version()` returns; the
 * returned view is valid for the lifetime of the program.
 */
inline std::string_view version() noexcept { return std::string_view(fasm_version()); }

/**
 * @brief Thrown by every fallible wrapper call.
 *
 * Carries the `fasm_status` and, for a parse error (`status() ==
 * Status::ParseError` or `Status::Utf8`), the 1 based line and 0 based
 * (code point) column, exactly as `fasm_error_line` / `fasm_error_column`
 * report them. `what()` is the same NUL terminated message `fasm.h`
 * documents (e.g. `"Parse error at 2:11 - ..."`).
 *
 * Constructing an `Error` consumes (frees) the `fasm_error *` it is built
 * from; callers never call `fasm_error_free` themselves when using this
 * wrapper.
 */
class Error : public std::runtime_error {
public:
    /**
     * @brief Builds an `Error` from a `fasm_error *`, freeing it.
     *
     * `err` may be `NULL` (some entry points, e.g. a `NULL` handle passed
     * to a query function, fail without ever allocating one); the result
     * is then a generic `Status::InvalidArg` error with a placeholder
     * message.
     */
    explicit Error(fasm_error *err) : Error(extract(err)) {}

    /** @brief The status code of the failure. */
    Status status() const noexcept { return status_; }

    /**
     * @brief Whether this error carries a source position.
     *
     * Only parse errors (`Status::ParseError`, `Status::Utf8`) do; `line()`
     * and `column()` are both 0 otherwise (matching `fasm_error_line` /
     * `fasm_error_column`, and indistinguishable from a genuine position
     * 0 in `column()` alone, hence this separate check on `line()`, which
     * is 1 based and so never legitimately 0).
     */
    bool has_position() const noexcept { return line_ != 0; }

    /** @brief 1 based line of the error, or 0 if `!has_position()`. */
    std::size_t line() const noexcept { return line_; }

    /** @brief 0 based, code point column of the error, or 0 if `!has_position()`. */
    std::size_t column() const noexcept { return column_; }

    /**
     * @brief `fasm_error_kind`: for the errors of the `fasm::xilinx`
     * functions, the name of the exception the reference Python tools
     * raise (e.g. `"prjxray.fasm_assembler.FasmLookupError"`, `"KeyError"`),
     * which their command line tools print before `what()`; the status
     * description otherwise.
     */
    const std::string &kind() const noexcept { return kind_; }

private:
    struct Fields {
        std::string message;
        Status status;
        std::size_t line;
        std::size_t column;
        std::string kind;
    };

    // Reads every field out of `err` and frees it before the base class
    // (std::runtime_error) and the members are constructed, so freeing
    // never races or interleaves with reading.
    static Fields extract(fasm_error *err) {
        Fields fields;
        if (err != nullptr) {
            const char *message = fasm_error_message(err);
            fields.message = message != nullptr ? std::string(message) : std::string();
            fields.status = from_c(fasm_error_status(err));
            fields.line = fasm_error_line(err);
            fields.column = fasm_error_column(err);
            const char *kind = fasm_error_kind(err);
            fields.kind = kind != nullptr ? std::string(kind) : std::string();
        } else {
            fields.message = "fasm: invalid argument (no error details available)";
            fields.status = Status::InvalidArg;
            fields.line = 0;
            fields.column = 0;
            fields.kind = fasm_status_string(FASM_ERR_INVALID_ARG);
        }
        fasm_error_free(err);
        return fields;
    }

    explicit Error(Fields fields)
        : std::runtime_error(std::move(fields.message)),
          status_(fields.status),
          line_(fields.line),
          column_(fields.column),
          kind_(std::move(fields.kind)) {}

    Status status_;
    std::size_t line_;
    std::size_t column_;
    std::string kind_;
};

namespace detail {

/** @brief Views a `fasm_str` as a `std::string_view` (empty, not dereferenced, for a `NULL` pointer). */
inline std::string_view to_sv(fasm_str s) noexcept {
    return s.ptr != nullptr ? std::string_view(s.ptr, s.len) : std::string_view();
}

/** @brief Builds a `fasm_str` view of `s` (`ptr` may be `NULL` when `s` is empty, as fasm.h allows). */
inline fasm_str to_fasm_str(std::string_view s) noexcept {
    fasm_str view;
    view.ptr = s.data();
    view.len = s.size();
    return view;
}

/** @brief Throws `Error(err)`. */
[[noreturn]] inline void throw_error(fasm_error *err) { throw Error(err); }

/** @brief Throws `Error(err)` unless `status == FASM_OK`. */
inline void check_status(fasm_status status, fasm_error *err) {
    if (status != FASM_OK) {
        throw_error(err);
    }
}

} // namespace detail

/**
 * @brief A minimal, C++17 compatible span over a contiguous read only
 * array, used for `File::push_line`'s annotation list.
 *
 * A stand-in for `std::span` (C++20), kept intentionally small: only what
 * `push_line` needs (construction from a `std::vector`, a brace-enclosed
 * initializer list, a C array or an explicit pointer + size).
 */
template <class T>
class Span {
public:
    /** @brief An empty span. */
    constexpr Span() noexcept = default;

    /** @brief A span over `[data, data + size)`. */
    constexpr Span(const T *data, std::size_t size) noexcept : data_(data), size_(size) {}

    /** @brief A span over a `std::vector`'s elements (must outlive the span). */
    Span(const std::vector<T> &v) noexcept : data_(v.data()), size_(v.size()) {}

    /** @brief A span over a braced initializer list (must outlive the span). */
    constexpr Span(std::initializer_list<T> list) noexcept
        : data_(list.begin()), size_(list.size()) {}

    /** @brief A span over a C array. */
    template <std::size_t N>
    constexpr Span(const T (&array)[N]) noexcept : data_(array), size_(N) {}

    constexpr const T *data() const noexcept { return data_; }
    constexpr std::size_t size() const noexcept { return size_; }
    constexpr bool empty() const noexcept { return size_ == 0; }
    constexpr const T *begin() const noexcept { return data_; }
    constexpr const T *end() const noexcept { return data_ + size_; }

private:
    const T *data_ = nullptr;
    std::size_t size_ = 0;
};

/**
 * @brief An owned, immutable UTF-8 string returned by the library (RAII
 * over `fasm_string *`).
 *
 * Move only (like `std::unique_ptr`): copying a `fasm_string` would need
 * an allocation the caller may not want, so it is not implicit.
 */
class String {
public:
    /** @brief An empty (null) string, matching all `fasm_*_free` accepting `NULL`. */
    String() noexcept = default;

    /** @brief Takes ownership of `s` (may be `NULL`). */
    explicit String(fasm_string *s) noexcept : ptr_(s) {}

    String(const String &) = delete;
    String &operator=(const String &) = delete;

    String(String &&other) noexcept : ptr_(other.ptr_) { other.ptr_ = nullptr; }

    String &operator=(String &&other) noexcept {
        if (this != &other) {
            fasm_string_free(ptr_);
            ptr_ = other.ptr_;
            other.ptr_ = nullptr;
        }
        return *this;
    }

    ~String() { fasm_string_free(ptr_); }

    /**
     * @brief A view of the text, valid as long as this `String` is (and not
     * moved from). The text may contain embedded NUL bytes; `view().size()`
     * (not `strlen`) is authoritative.
     */
    std::string_view view() const noexcept {
        return ptr_ != nullptr ? std::string_view(fasm_string_data(ptr_), fasm_string_len(ptr_))
                                : std::string_view();
    }

    /** @brief A copy of the text as a `std::string`. */
    std::string str() const { return std::string(view()); }

    /** @brief The underlying `fasm_string *`, still owned by this `String`. */
    const fasm_string *raw() const noexcept { return ptr_; }

private:
    fasm_string *ptr_ = nullptr;
};

namespace detail {

/** @brief Wraps `s` in a `String`, or throws `Error(err)` if `s` is `NULL`. */
inline String make_string(fasm_string *s, fasm_error *err) {
    if (s == nullptr) {
        throw_error(err);
    }
    return String(s);
}

} // namespace detail

/**
 * @brief Read only access to the (arbitrary width) value of a `SetFeature`.
 *
 * A non-owning view over a `const fasm_set_feature *`: valid exactly as
 * long as the `SetFeature` (and the `Line` / `File` it came from) it was
 * obtained from.
 */
class Value {
public:
    /** @brief Wraps `sf` (may be `NULL`, in which case every accessor reports the value 0). */
    explicit Value(const fasm_set_feature *sf) noexcept : sf_(sf) {}

    /** @brief Bits needed to hold the value: 0 for the value 0, else one more than its highest set bit. */
    std::uint32_t bit_length() const noexcept { return fasm_set_feature_value_bits(sf_); }

    /** @brief The value if it fits in 64 bits, else `std::nullopt`. */
    std::optional<std::uint64_t> u64() const noexcept {
        std::uint64_t value = 0;
        if (fasm_set_feature_value_u64(sf_, &value)) {
            return value;
        }
        return std::nullopt;
    }

    /** @brief Bit `index` (0 = least significant); `false` beyond `bit_length()`. */
    bool bit(std::uint32_t index) const noexcept { return fasm_set_feature_value_bit(sf_, index); }

    /** @brief The value as little endian bytes, exactly `(bit_length() + 7) / 8` long (empty for 0). */
    std::vector<std::uint8_t> bytes_le() const {
        std::size_t n = fasm_set_feature_value_bytes_le(sf_, nullptr, 0);
        std::vector<std::uint8_t> buf(n);
        if (n > 0) {
            fasm_set_feature_value_bytes_le(sf_, buf.data(), n);
        }
        return buf;
    }

    /**
     * @brief The value written as digits in `radix` (2, 8, 10 or 16), no
     * prefix or width, no leading zeros (`"0"` for 0); hex digits upper
     * case if `uppercase`.
     *
     * @throws Error `Status::InvalidArg` if `radix` is not one of 2, 8, 10, 16.
     */
    std::string to_string(std::uint32_t radix = 10, bool uppercase = false) const {
        fasm_error *err = nullptr;
        fasm_string *s = fasm_set_feature_value_to_string(sf_, radix, uppercase, &err);
        return detail::make_string(s, err).str();
    }

private:
    const fasm_set_feature *sf_;
};

/**
 * @brief The `SetFasmFeature` of a `Line`: a feature name, an optional
 * `[end:start]` address, a value and the format it was written in.
 *
 * A non-owning view over a `const fasm_set_feature *`: valid as long as
 * its `Line` is.
 */
class SetFeature {
public:
    /** @brief Wraps `sf` (must not be `NULL`: obtain a `SetFeature` only through `Line::set_feature()`). */
    explicit SetFeature(const fasm_set_feature *sf) noexcept : sf_(sf) {}

    /**
     * @brief Copies the feature name into `buf` with `snprintf` semantics
     * (as `fasm_set_feature_name`): writes at most `buf_len - 1` bytes plus
     * a NUL, and returns the full length (name was truncated if the
     * return value is `>= buf_len`). No allocation.
     */
    std::size_t name_into(char *buf, std::size_t buf_len) const noexcept {
        return fasm_set_feature_name(sf_, buf, buf_len);
    }

    /** @brief The feature name, e.g. `"CLBLL_L_X12Y124.SLICEL_X0.BLUT.INIT"`. */
    std::string name() const {
        std::size_t len = fasm_set_feature_name_len(sf_);
        std::string s(len, '\0');
        if (len > 0) {
            name_into(s.data(), len + 1);
        }
        return s;
    }

    /** @brief The start (low) address bit, if the feature has an address (`FEATURE[start]` or `FEATURE[end:start]`). */
    std::optional<std::uint32_t> start() const noexcept {
        if (!fasm_set_feature_has_start(sf_)) {
            return std::nullopt;
        }
        return fasm_set_feature_start(sf_);
    }

    /** @brief The end (high) address bit, if the feature has an end (`FEATURE[end:start]`). */
    std::optional<std::uint32_t> end() const noexcept {
        if (!fasm_set_feature_has_end(sf_)) {
            return std::nullopt;
        }
        return fasm_set_feature_end(sf_);
    }

    /** @brief The format the value was written in, or `std::nullopt` for an implicit value (`FEATURE`, `FEATURE[3]`). */
    std::optional<ValueFormat> value_format() const noexcept {
        return from_c(fasm_set_feature_value_format(sf_));
    }

    /** @brief Address width in bits: 1 without an address or with a single bit address, `end - start + 1` for a range. */
    std::uint32_t width() const noexcept { return fasm_set_feature_width(sf_); }

    /** @brief Read only access to the value. */
    Value value() const noexcept { return Value(sf_); }

    /**
     * @brief Renders this feature, e.g. `"A.B[7:0] = 8'hFF"`.
     *
     * @param canonical With `true`, requires the feature to already be in
     * canonical form (width 1, no end address, start address 0 if any,
     * value written), throwing `Status::Output` otherwise.
     */
    std::string to_string(bool canonical = false) const {
        fasm_error *err = nullptr;
        fasm_string *s = fasm_set_feature_to_string(sf_, canonical, &err);
        return detail::make_string(s, err).str();
    }

    /** @brief The underlying `fasm_set_feature *`. */
    const fasm_set_feature *raw() const noexcept { return sf_; }

private:
    const fasm_set_feature *sf_;
};

/**
 * @brief One `name = "value"` annotation, as two borrowed (from a `Line`)
 * or caller owned (as input to `File::push_line`) string views.
 */
struct Annotation {
    /** @brief The annotation name. */
    std::string_view name;
    /** @brief The annotation value (raw text between the quotes; possibly empty). */
    std::string_view value;
};

/**
 * @brief One line of a FASM model: an optional `SetFeature`, optional
 * annotations and an optional comment.
 *
 * A non-owning view over a `const fasm_line *`: valid as long as its
 * `File` is not freed or modified (`File::push_line`), or, for a line
 * handed to `File::parse_each`, only during that call (do not store a
 * `Line` obtained that way).
 */
class Line {
public:
    /** @brief Wraps `line` (must not be `NULL`: obtain a `Line` only from a `File` or a streaming callback). */
    explicit Line(const fasm_line *line) noexcept : line_(line) {}

    /** @brief This line's `SetFeature`, if it has one. */
    std::optional<SetFeature> set_feature() const noexcept {
        const fasm_set_feature *sf = fasm_line_set_feature(line_);
        if (sf == nullptr) {
            return std::nullopt;
        }
        return SetFeature(sf);
    }

    /**
     * @brief A random access, read only range of this line's `Annotation`s
     * (`{ .a = "x", .b = "y" }`), in file order. Empty if the line has no
     * `{ ... }` block.
     */
    class AnnotationRange {
    public:
        /** @brief A random access iterator dereferencing to an `Annotation` (by value). */
        class iterator {
        public:
            using iterator_category = std::random_access_iterator_tag;
            using value_type = Annotation;
            using difference_type = std::ptrdiff_t;
            using pointer = void;
            using reference = Annotation;

            iterator() noexcept = default;

            Annotation operator*() const noexcept {
                fasm_annotation a{};
                fasm_line_annotation(line_, index_, &a);
                return Annotation{detail::to_sv(a.name), detail::to_sv(a.value)};
            }

            iterator &operator++() noexcept {
                ++index_;
                return *this;
            }
            iterator operator++(int) noexcept {
                iterator tmp = *this;
                ++*this;
                return tmp;
            }
            iterator &operator--() noexcept {
                --index_;
                return *this;
            }
            iterator operator--(int) noexcept {
                iterator tmp = *this;
                --*this;
                return tmp;
            }
            iterator &operator+=(difference_type n) noexcept {
                index_ = static_cast<std::size_t>(static_cast<difference_type>(index_) + n);
                return *this;
            }
            iterator &operator-=(difference_type n) noexcept { return *this += -n; }
            iterator operator+(difference_type n) const noexcept {
                iterator tmp = *this;
                tmp += n;
                return tmp;
            }
            iterator operator-(difference_type n) const noexcept {
                iterator tmp = *this;
                tmp -= n;
                return tmp;
            }
            difference_type operator-(const iterator &other) const noexcept {
                return static_cast<difference_type>(index_) -
                       static_cast<difference_type>(other.index_);
            }
            Annotation operator[](difference_type n) const noexcept { return *(*this + n); }
            bool operator==(const iterator &other) const noexcept {
                return line_ == other.line_ && index_ == other.index_;
            }
            bool operator!=(const iterator &other) const noexcept { return !(*this == other); }
            bool operator<(const iterator &other) const noexcept { return index_ < other.index_; }
            bool operator>(const iterator &other) const noexcept { return other < *this; }
            bool operator<=(const iterator &other) const noexcept { return !(other < *this); }
            bool operator>=(const iterator &other) const noexcept { return !(*this < other); }

        private:
            friend class AnnotationRange;
            iterator(const fasm_line *line, std::size_t index) noexcept
                : line_(line), index_(index) {}
            const fasm_line *line_ = nullptr;
            std::size_t index_ = 0;
        };

        explicit AnnotationRange(const fasm_line *line) noexcept : line_(line) {}

        /** @brief Number of annotations. */
        std::size_t size() const noexcept { return fasm_line_annotation_count(line_); }
        bool empty() const noexcept { return size() == 0; }
        Annotation operator[](std::size_t index) const noexcept {
            fasm_annotation a{};
            fasm_line_annotation(line_, index, &a);
            return Annotation{detail::to_sv(a.name), detail::to_sv(a.value)};
        }
        iterator begin() const noexcept { return iterator(line_, 0); }
        iterator end() const noexcept { return iterator(line_, size()); }

    private:
        const fasm_line *line_;
    };

    /** @brief This line's annotations (empty range if it has none). */
    AnnotationRange annotations() const noexcept { return AnnotationRange(line_); }

    /**
     * @brief This line's comment text (everything after `#`, verbatim), if
     * it has one. A bare `#` gives an empty (but present) view.
     */
    std::optional<std::string_view> comment() const noexcept {
        fasm_str out;
        if (fasm_line_comment(line_, &out)) {
            return detail::to_sv(out);
        }
        return std::nullopt;
    }

    /**
     * @brief Renders this line, e.g. `"A.B[7:0] = 8'hFF { .x = \"y\" } # c"`.
     *
     * @param canonical With `true`, the canonical lines of this line's set
     * feature (none, an empty string, if it has none or the value 0), in
     * bit order, not deduplicated; joined with `\n` if there is more than
     * one (matches `fasm_line_to_string`).
     */
    std::string to_string(bool canonical = false) const {
        fasm_error *err = nullptr;
        fasm_string *s = fasm_line_to_string(line_, canonical, &err);
        return detail::make_string(s, err).str();
    }

    /** @brief The underlying `fasm_line *`. */
    const fasm_line *raw() const noexcept { return line_; }

private:
    const fasm_line *line_;
};

/**
 * @brief Input description of a `SetFasmFeature`, for `File::push_line`.
 *
 * Mirrors `fasm_set_feature_spec`. Like the C API (and the Python model),
 * `name` is not checked against the FASM grammar.
 */
struct SetFeatureSpec {
    /** @brief The feature name, e.g. `"CLBLL_L_X12Y124.SLICEL_X0.BLUT.INIT"`. */
    std::string_view name;
    /** @brief Start (low) address bit, if any (`FEATURE[start]` / `FEATURE[end:start]`). */
    std::optional<std::uint32_t> start;
    /** @brief End (high) address bit, if any (`FEATURE[end:start]`; requires `start`). */
    std::optional<std::uint32_t> end;
    /** @brief The value as little endian bytes (empty is the value 0). */
    std::vector<std::uint8_t> value_le;
    /** @brief The format to write the value in, or `std::nullopt` for no written value (an implicit 1). */
    std::optional<ValueFormat> format;

    /** @brief A `SetFeatureSpec` with the value given as a `uint64_t` rather than bytes. */
    static SetFeatureSpec with_u64(std::string_view name, std::uint64_t value,
                                   std::optional<ValueFormat> format = ValueFormat::Plain) {
        SetFeatureSpec spec;
        spec.name = name;
        spec.format = format;
        spec.value_le.resize(sizeof(value));
        for (std::size_t i = 0; i < sizeof(value); ++i) {
            spec.value_le[i] = static_cast<std::uint8_t>(value >> (8 * i));
        }
        return spec;
    }
};

/**
 * @brief A FASM model: an ordered list of `Line`s.
 *
 * Owns a `fasm_file *` (RAII, move only, like `std::unique_ptr`): copying
 * a whole model would be an easy to miss deep copy, so it is not implicit
 * (build a new `File` and `push_line` into it, or `merge_and_sort` a fresh
 * copy, instead).
 */
class File {
public:
    /** @brief A random access iterator over a `File`'s `Line`s (dereferences to `Line` by value). */
    class iterator {
    public:
        using iterator_category = std::random_access_iterator_tag;
        using value_type = Line;
        using difference_type = std::ptrdiff_t;
        using pointer = void;
        using reference = Line;

        iterator() noexcept = default;

        Line operator*() const noexcept { return Line(fasm_file_line(file_, index_)); }

        iterator &operator++() noexcept {
            ++index_;
            return *this;
        }
        iterator operator++(int) noexcept {
            iterator tmp = *this;
            ++*this;
            return tmp;
        }
        iterator &operator--() noexcept {
            --index_;
            return *this;
        }
        iterator operator--(int) noexcept {
            iterator tmp = *this;
            --*this;
            return tmp;
        }
        iterator &operator+=(difference_type n) noexcept {
            index_ = static_cast<std::size_t>(static_cast<difference_type>(index_) + n);
            return *this;
        }
        iterator &operator-=(difference_type n) noexcept { return *this += -n; }
        iterator operator+(difference_type n) const noexcept {
            iterator tmp = *this;
            tmp += n;
            return tmp;
        }
        iterator operator-(difference_type n) const noexcept {
            iterator tmp = *this;
            tmp -= n;
            return tmp;
        }
        difference_type operator-(const iterator &other) const noexcept {
            return static_cast<difference_type>(index_) - static_cast<difference_type>(other.index_);
        }
        Line operator[](difference_type n) const noexcept { return *(*this + n); }
        bool operator==(const iterator &other) const noexcept {
            return file_ == other.file_ && index_ == other.index_;
        }
        bool operator!=(const iterator &other) const noexcept { return !(*this == other); }
        bool operator<(const iterator &other) const noexcept { return index_ < other.index_; }
        bool operator>(const iterator &other) const noexcept { return other < *this; }
        bool operator<=(const iterator &other) const noexcept { return !(other < *this); }
        bool operator>=(const iterator &other) const noexcept { return !(*this < other); }

    private:
        friend class File;
        iterator(const fasm_file *file, std::size_t index) noexcept : file_(file), index_(index) {}
        const fasm_file *file_ = nullptr;
        std::size_t index_ = 0;
    };

    /** @brief A callback deciding whether a feature is a "zero" feature (see `merge_and_sort`). An empty `ZeroFn` means "none". */
    using ZeroFn = std::function<bool(std::string_view feature)>;
    /** @brief A callback computing a feature group's sort key (see `merge_and_sort`). An empty `SortKeyFn` means "none" (sort by name). */
    using SortKeyFn = std::function<std::int64_t(std::string_view group_id)>;

    /** @brief A new, empty file. */
    File() noexcept : ptr_(fasm_file_new()) {}

    /** @brief Takes ownership of an existing `fasm_file *` (may be `NULL`). */
    explicit File(fasm_file *file) noexcept : ptr_(file) {}

    File(const File &) = delete;
    File &operator=(const File &) = delete;

    File(File &&other) noexcept : ptr_(other.ptr_) { other.ptr_ = nullptr; }

    File &operator=(File &&other) noexcept {
        if (this != &other) {
            fasm_file_free(ptr_);
            ptr_ = other.ptr_;
            other.ptr_ = nullptr;
        }
        return *this;
    }

    ~File() { fasm_file_free(ptr_); }

    /**
     * @brief Parses FASM text (Python: `fasm.parse_fasm_string`).
     * @throws Error `Status::ParseError` / `Status::Utf8`, with line/column.
     */
    static File parse(std::string_view text) {
        fasm_file *out = nullptr;
        fasm_error *err = nullptr;
        fasm_status status = fasm_parse_string(text.data(), text.size(), &out, &err);
        detail::check_status(status, err);
        return File(out);
    }

    /**
     * @brief Reads and parses a FASM file (Python: `fasm.parse_fasm_filename`).
     *
     * `path` reaches `fasm_parse_file` as `path.string().c_str()`: on
     * POSIX, `fasm.h` accepts any byte sequence for a path (matching
     * `std::filesystem::path`'s native, encoding-agnostic representation
     * there) and this is exact; on Windows, `fasm.h` requires UTF-8 but
     * `path.string()` instead uses the active code page, so a path with
     * characters outside it would not round trip correctly (not exercised
     * by this wrapper's tests, which run on POSIX only).
     *
     * @throws Error `Status::Io` if the file cannot be read (no position),
     * else as `parse`.
     */
    static File parse_file(const std::filesystem::path &path) {
        fasm_file *out = nullptr;
        fasm_error *err = nullptr;
        std::string p = path.string();
        fasm_status status = fasm_parse_file(p.c_str(), &out, &err);
        detail::check_status(status, err);
        return File(out);
    }

    /**
     * @brief Parses `text` without building a `File`, calling `f(Line,
     * line_number)` for each line (Python-side equivalent of
     * `fasm_parse_string_cb`), which avoids holding the whole model in
     * memory.
     *
     * `f` may return `bool` (`false` stops parsing early, like the C
     * callback) or `void` (equivalent to always returning `true`). The
     * `Line` passed to `f` is valid only during that call.
     *
     * If `f` throws, the exception is caught inside the C callback
     * trampoline (throwing through the Rust `extern "C"` boundary is
     * undefined behaviour), parsing is stopped, and the same exception is
     * rethrown here once `fasm_parse_string_cb` has returned.
     *
     * @throws Error `Status::ParseError` / `Status::Utf8` for lines up to
     * and including a syntax error not pre-empted by `f` returning `false`
     * or throwing.
     */
    template <class F>
    static void parse_each(std::string_view text, F &&f) {
        parse_each_impl(std::forward<F>(f), [&](fasm_line_callback trampoline, void *user,
                                                 fasm_error **err) {
            return fasm_parse_string_cb(text.data(), text.size(), trampoline, user, err);
        });
    }

    /** @brief Like `parse_each`, but reads `path` first (see `parse_file`). */
    template <class F>
    static void parse_each_file(const std::filesystem::path &path, F &&f) {
        std::string p = path.string();
        parse_each_impl(std::forward<F>(f), [&](fasm_line_callback trampoline, void *user,
                                                 fasm_error **err) {
            return fasm_parse_file_cb(p.c_str(), trampoline, user, err);
        });
    }

    /** @brief Number of lines. */
    std::size_t size() const noexcept { return fasm_file_line_count(ptr_); }
    bool empty() const noexcept { return size() == 0; }

    /** @brief Line `index` (0 based); no bounds check (mirrors `fasm_file_line`, `NULL` beyond the end becomes an unusable `Line`). */
    Line operator[](std::size_t index) const noexcept { return Line(fasm_file_line(ptr_, index)); }

    /** @brief Line `index`, bounds checked. @throws std::out_of_range if `index >= size()`. */
    Line at(std::size_t index) const {
        if (index >= size()) {
            throw std::out_of_range("fasm::File::at: index out of range");
        }
        return (*this)[index];
    }

    iterator begin() const noexcept { return iterator(ptr_, 0); }
    iterator end() const noexcept { return iterator(ptr_, size()); }

    /**
     * @brief Renders every line as FASM text (Python:
     * `fasm.fasm_tuple_to_string`); an empty file gives `"\n"`.
     * @param canonical See `Line::to_string`; applied to every line, then
     * the whole file's lines sorted and deduplicated.
     */
    std::string to_string(bool canonical = false) const {
        fasm_error *err = nullptr;
        fasm_string *s = fasm_file_to_string(ptr_, canonical, &err);
        return detail::make_string(s, err).str();
    }

    /**
     * @brief Groups and sorts the lines into a new `File` (Python:
     * `fasm.output.merge_and_sort(model)`), leaving this file unchanged.
     */
    File merge_and_sort() const {
        fasm_error *err = nullptr;
        fasm_file *out = fasm_file_merge_and_sort(ptr_, &err);
        if (out == nullptr) {
            detail::throw_error(err);
        }
        return File(out);
    }

    /**
     * @brief `merge_and_sort()` with Python's optional callbacks: a
     * feature group all of whose features `zero_fn` answers `true` for is
     * dropped, and `sort_key_fn` gives an explicit sort key per group
     * (called exactly once per group; see `fasm_file_merge_and_sort_ex`).
     * An empty (default constructed) `ZeroFn` / `SortKeyFn` means "none".
     *
     * If a callback throws, the exception is caught inside its C
     * trampoline (see the file level documentation) and rethrown here
     * once `fasm_file_merge_and_sort_ex` returns. Unlike `parse_each`,
     * `zero_fn` / `sort_key_fn` have no early-stop protocol, so they keep
     * being called for the rest of the model after one has thrown; only
     * the FIRST exception raised is kept and rethrown, every later one is
     * discarded.
     */
    File merge_and_sort(const ZeroFn &zero_fn, const SortKeyFn &sort_key_fn) const {
        struct Context {
            const ZeroFn *zero;
            const SortKeyFn *sort_key;
            std::exception_ptr pending;
        } context{zero_fn ? &zero_fn : nullptr, sort_key_fn ? &sort_key_fn : nullptr, nullptr};

        static const fasm_zero_fn zero_trampoline =
            +[](const char *feature, std::size_t len, void *user) -> bool {
            auto *ctx = static_cast<Context *>(user);
            try {
                return (*ctx->zero)(std::string_view(feature, len));
            } catch (...) {
                // fasm_zero_fn has no early-stop protocol: the C side keeps
                // calling this trampoline for the rest of the model even
                // after an exception, so only the FIRST one is kept (a
                // later one is silently discarded, matching "the first
                // exception raised wins", not "the last one seen").
                if (!ctx->pending) {
                    ctx->pending = std::current_exception();
                }
                return true; // return value is otherwise unused (see above).
            }
        };
        static const fasm_sort_key_fn sort_key_trampoline =
            +[](const char *group_id, std::size_t len, void *user) -> std::int64_t {
            auto *ctx = static_cast<Context *>(user);
            try {
                return (*ctx->sort_key)(std::string_view(group_id, len));
            } catch (...) {
                // Same "first exception wins" rule as zero_trampoline above:
                // fasm_sort_key_fn also has no early-stop protocol.
                if (!ctx->pending) {
                    ctx->pending = std::current_exception();
                }
                return 0; // return value is otherwise unused (see above).
            }
        };

        fasm_error *err = nullptr;
        fasm_file *out = fasm_file_merge_and_sort_ex(
            ptr_, context.zero != nullptr ? zero_trampoline : nullptr,
            context.sort_key != nullptr ? sort_key_trampoline : nullptr, &context, &err);
        if (context.pending) {
            fasm_file_free(out);
            fasm_error_free(err);
            std::rethrow_exception(context.pending);
        }
        if (out == nullptr) {
            detail::throw_error(err);
        }
        return File(out);
    }

    /**
     * @brief Appends a line with a `SetFeature` (Python: appending a
     * `fasm.FasmLine` with a `set_feature` to a model).
     *
     * @param feature The feature to set; validated like
     * `fasm.SetFasmFeature` (end without start, end before start, or a
     * value wider than the address throw `Status::InvalidArg`).
     * @param annotations This line's `{ ... }` annotations, if any.
     * @param comment This line's `# ...` comment text, if any (an empty
     * but present `comment` is a bare `#`).
     *
     * Appending invalidates every `Line` / `SetFeature` view previously
     * obtained from this `File` (the underlying line array may be
     * reallocated), exactly as `fasm_file_push_line` documents.
     */
    void push_line(const SetFeatureSpec &feature, Span<Annotation> annotations = {},
                   std::optional<std::string_view> comment = std::nullopt) {
        fasm_set_feature_spec spec{};
        spec.feature = detail::to_fasm_str(feature.name);
        spec.has_start = feature.start.has_value();
        spec.start = feature.start.value_or(0);
        spec.has_end = feature.end.has_value();
        spec.end = feature.end.value_or(0);
        spec.value_le = feature.value_le.empty() ? nullptr : feature.value_le.data();
        spec.value_len = feature.value_le.size();
        spec.value_format = to_c(feature.format);
        push_line_raw(&spec, annotations, comment);
    }

    /** @brief Appends a line with only a `# comment` (no feature, no annotations). */
    void push_comment(std::string_view comment) { push_line_raw(nullptr, {}, comment); }

    /** @brief Appends a line with only `{ annotations }` (and, optionally, a comment); no feature. */
    void push_annotations(Span<Annotation> annotations,
                          std::optional<std::string_view> comment = std::nullopt) {
        push_line_raw(nullptr, annotations, comment);
    }

    /** @brief Appends a blank line (no feature, no annotations, no comment). */
    void push_blank_line() { push_line_raw(nullptr, {}, std::nullopt); }

    /** @brief The underlying `fasm_file *`, still owned by this `File`. */
    fasm_file *raw() noexcept { return ptr_; }
    const fasm_file *raw() const noexcept { return ptr_; }

private:
    void push_line_raw(const fasm_set_feature_spec *spec, Span<Annotation> annotations,
                       std::optional<std::string_view> comment) {
        std::vector<fasm_annotation> c_annotations;
        c_annotations.reserve(annotations.size());
        for (const Annotation &a : annotations) {
            fasm_annotation ca;
            ca.name = detail::to_fasm_str(a.name);
            ca.value = detail::to_fasm_str(a.value);
            c_annotations.push_back(ca);
        }
        fasm_str c_comment{};
        const fasm_str *c_comment_ptr = nullptr;
        if (comment.has_value()) {
            c_comment = detail::to_fasm_str(*comment);
            c_comment_ptr = &c_comment;
        }
        fasm_error *err = nullptr;
        fasm_status status =
            fasm_file_push_line(ptr_, spec, c_annotations.empty() ? nullptr : c_annotations.data(),
                                c_annotations.size(), c_comment_ptr, &err);
        detail::check_status(status, err);
    }

    // Shared implementation of parse_each / parse_each_file: `call` invokes
    // the right fasm_parse_*_cb with the trampoline and context it is
    // given. See the file level documentation for the exception rule.
    template <class F, class Call>
    static void parse_each_impl(F &&f, Call &&call) {
        struct Context {
            std::remove_reference_t<F> &func;
            std::exception_ptr pending;
        } context{f, nullptr};

        auto trampoline = [](const fasm_line *line, std::size_t line_number, void *user) -> bool {
            auto *ctx = static_cast<Context *>(user);
            try {
                return invoke_line_callback(ctx->func, Line(line), line_number);
            } catch (...) {
                // Returning false does stop fasm_parse_*_cb from calling
                // this trampoline again (unlike the merge_and_sort
                // callbacks above), so only one exception can ever be
                // stored here; guarded the same way regardless, for
                // consistency and in case that early-stop contract ever
                // changes.
                if (!ctx->pending) {
                    ctx->pending = std::current_exception();
                }
                return false;
            }
        };

        fasm_error *err = nullptr;
        fasm_status status = call(trampoline, &context, &err);
        if (context.pending) {
            fasm_error_free(err);
            std::rethrow_exception(context.pending);
        }
        detail::check_status(status, err);
    }

    // Calls f(line, line_number); if f returns bool, that value is used to
    // continue/stop, otherwise (a void returning f) parsing always
    // continues.
    template <class Fn>
    static bool invoke_line_callback(Fn &f, const Line &line, std::size_t line_number) {
        if constexpr (std::is_invocable_r_v<bool, Fn &, const Line &, std::size_t>) {
            return f(line, line_number);
        } else {
            f(line, line_number);
            return true;
        }
    }

    fasm_file *ptr_ = nullptr;
};

} // namespace fasm

/**
 * @brief C++ wrappers of the `fasm_xilinx_*` functions: prjxray-db /
 * prjuray-db databases, FASM -> frames, `.frm` files and `.bit`
 * bitstreams for Xilinx Series7, UltraScale and UltraScale+ parts
 * (docs/rewrite/DESIGN-capi.md, "Xilinx").
 *
 * `Database`, `Assembler`, `Frames` and `Part` own their C object (move
 * only). Failures throw `fasm::Error` (see `Error::kind()`). Warning
 * callbacks (`Fasm2FramesOptions::warning`, `Frames::parse_frm`) go
 * through the same exception trampoline as the core wrapper: an
 * exception they throw is rethrown after the C call returns.
 */
namespace fasm::xilinx {

/** @brief Mirrors `fasm_xilinx_architecture`. */
enum class Architecture : int {
    Series7 = FASM_XILINX_SERIES7,
    UltraScale = FASM_XILINX_ULTRASCALE,
    UltraScalePlus = FASM_XILINX_ULTRASCALE_PLUS,
};

/** @brief Mirrors `fasm_xilinx_bitstream_format`. */
enum class BitstreamFormat : std::int32_t {
    Default = FASM_XILINX_FORMAT_DEFAULT,
    Series7 = FASM_XILINX_FORMAT_SERIES7,
    UltraScale = FASM_XILINX_FORMAT_ULTRASCALE,
    UltraScalePlus = FASM_XILINX_FORMAT_ULTRASCALE_PLUS,
    PrjxrayUltraScale = FASM_XILINX_FORMAT_PRJXRAY_ULTRASCALE,
    PrjxrayUltraScalePlus = FASM_XILINX_FORMAT_PRJXRAY_ULTRASCALE_PLUS,
};

/** @brief A warning of the reference tools (e.g. a bit beyond its frame). */
using WarningFn = std::function<void(std::string_view)>;

namespace detail {

/** @brief The context of the warning trampoline. */
struct WarningContext {
    const WarningFn *fn;
    std::exception_ptr pending;
};

/**
 * @brief A `fasm_xilinx_warning_fn` calling a `WarningFn`; never unwinds
 * (the first exception is stored in the context, later warnings are
 * dropped).
 */
inline void warning_trampoline(const char *message, std::size_t len, void *user) noexcept {
    auto *ctx = static_cast<WarningContext *>(user);
    if (ctx->pending) {
        return;
    }
    try {
        (*ctx->fn)(std::string_view(message, len));
    } catch (...) {
        ctx->pending = std::current_exception();
    }
}

/** @brief Rethrows the callback's exception, else throws `Error(err)` for a failure. */
inline void finish(const WarningContext &ctx, fasm_status status, fasm_error *err) {
    if (ctx.pending) {
        fasm_error_free(err);
        std::rethrow_exception(ctx.pending);
    }
    ::fasm::detail::check_status(status, err);
}

/** @brief Owns a C object, freed with `Free` (move only). */
template <class T, void (*Free)(T *)>
class Handle {
public:
    Handle() noexcept = default;
    explicit Handle(T *ptr) noexcept : ptr_(ptr) {}
    Handle(const Handle &) = delete;
    Handle &operator=(const Handle &) = delete;
    Handle(Handle &&other) noexcept : ptr_(other.ptr_) { other.ptr_ = nullptr; }
    Handle &operator=(Handle &&other) noexcept {
        if (this != &other) {
            Free(ptr_);
            ptr_ = other.ptr_;
            other.ptr_ = nullptr;
        }
        return *this;
    }
    ~Handle() { Free(ptr_); }
    T *get() const noexcept { return ptr_; }

private:
    T *ptr_ = nullptr;
};

/** @brief A C path string for `path` (exact on POSIX, see `File::parse_file`). */
inline std::string path_string(const std::filesystem::path &path) { return path.string(); }

} // namespace detail

/** @brief A bit a feature sets (`value`) or clears (`fasm_xilinx_bit`). */
using Bit = fasm_xilinx_bit;

/** @brief The result of `Database::lookup` (`fasm_xilinx_feature_info` and its bits). */
struct FeatureInfo {
    /** @brief A pseudo PIP: valid, sets no bits. */
    bool pseudo_pip = false;
    /** @brief The bus: 0 `CLB_IO_CLK`, 1 `BLOCK_RAM`, 2 `CFG_CLB`, -1 for a pseudo PIP. */
    std::int32_t block_type = -1;
    /** @brief The first frame of the bus of the tile. */
    std::uint32_t base_address = 0;
    /** @brief The number of frames of the bus of the tile. */
    std::uint32_t frame_count = 0;
    /** @brief The effective word offset. */
    std::int64_t offset = 0;
    /** @brief The bits (placeable ones only). */
    std::vector<Bit> bits;
};

/**
 * @brief A prjxray-db / prjuray-db database family opened for one part
 * (`fasm_xilinx_database`; immutable, shareable between threads).
 */
class Database {
public:
    /** @brief `fasm_xilinx_database_open`: loads the text files. */
    static Database open(const std::filesystem::path &db_root,
                         const std::optional<std::string> &part = std::nullopt) {
        fasm_xilinx_database *out = nullptr;
        fasm_error *err = nullptr;
        std::string root = detail::path_string(db_root);
        fasm_status status = fasm_xilinx_database_open(
            root.c_str(), part ? part->c_str() : nullptr, &out, &err);
        ::fasm::detail::check_status(status, err);
        return Database(out);
    }

    /**
     * @brief `fasm_xilinx_database_open_cached`: through the binary cache in
     * `cache_dir`, or with the settings of the command line tools
     * (`$FASM_XDB_CACHE`, ...) when `cache_dir` is `std::nullopt`.
     */
    static Database open_cached(const std::filesystem::path &db_root,
                                const std::optional<std::string> &part,
                                const std::optional<std::filesystem::path> &cache_dir =
                                    std::nullopt) {
        fasm_xilinx_database *out = nullptr;
        fasm_error *err = nullptr;
        std::string root = detail::path_string(db_root);
        std::string dir = cache_dir ? detail::path_string(*cache_dir) : std::string();
        fasm_status status = fasm_xilinx_database_open_cached(
            root.c_str(), part ? part->c_str() : nullptr, cache_dir ? dir.c_str() : nullptr,
            &out, &err);
        ::fasm::detail::check_status(status, err);
        return Database(out);
    }

    /** @brief The architecture. */
    Architecture architecture() const noexcept {
        return static_cast<Architecture>(fasm_xilinx_database_architecture(get()));
    }

    /** @brief 32-bit words per frame (101, 123 or 93). */
    std::size_t words_per_frame() const noexcept {
        return fasm_xilinx_database_words_per_frame(get());
    }

    /** @brief The part, or `std::nullopt` for a database opened without one. */
    std::optional<std::string_view> part() const noexcept {
        const char *p = fasm_xilinx_database_part(get());
        return p != nullptr ? std::optional<std::string_view>(p) : std::nullopt;
    }

    /** @brief `fasm_xilinx_database_lookup`: bit `address` of the feature `feature`. */
    FeatureInfo lookup(std::string_view feature, std::uint32_t address = 0) const {
        fasm_xilinx_feature_info info{};
        fasm_error *err = nullptr;
        fasm_status status = fasm_xilinx_database_lookup(get(), feature.data(), feature.size(),
                                                         address, &info, nullptr, 0, &err);
        ::fasm::detail::check_status(status, err);
        FeatureInfo result;
        result.bits.resize(info.bit_count);
        status = fasm_xilinx_database_lookup(get(), feature.data(), feature.size(), address,
                                             &info, result.bits.data(), result.bits.size(), &err);
        ::fasm::detail::check_status(status, err);
        result.pseudo_pip = info.pseudo_pip;
        result.block_type = info.block_type;
        result.base_address = info.base_address;
        result.frame_count = info.frame_count;
        result.offset = info.offset;
        return result;
    }

    /** @brief The underlying `fasm_xilinx_database *`, still owned by this object. */
    const fasm_xilinx_database *get() const noexcept { return handle_.get(); }

private:
    explicit Database(fasm_xilinx_database *ptr) noexcept : handle_(ptr) {}

    detail::Handle<fasm_xilinx_database, fasm_xilinx_database_free> handle_;
};

/** @brief One frame of `Frames`: its address and `words_per_frame` words (borrowed). */
struct FrameView {
    std::uint32_t address;
    ::fasm::Span<std::uint32_t> words;
};

/**
 * @brief Configuration frames: address -> words, in ascending address
 * order (`fasm_xilinx_frames`).
 */
class Frames {
public:
    /** @brief No frames, of `words_per_frame` words each (101, 123 or 93). */
    explicit Frames(std::size_t words_per_frame)
        : handle_(fasm_xilinx_frames_new(words_per_frame)) {
        if (handle_.get() == nullptr) {
            throw std::invalid_argument("fasm::xilinx::Frames: words_per_frame must not be 0");
        }
    }

    /** @brief Takes ownership of `ptr` (not `NULL`). */
    static Frames from_raw(fasm_xilinx_frames *ptr) noexcept { return Frames(Adopt{}, ptr); }

    /** @brief The number of frames. */
    std::size_t size() const noexcept { return fasm_xilinx_frames_count(get()); }

    /** @brief 32-bit words per frame. */
    std::size_t words_per_frame() const noexcept {
        return fasm_xilinx_frames_words_per_frame(get());
    }

    /** @brief Frame `index` (in address order); valid until this object is modified. */
    FrameView operator[](std::size_t index) const noexcept {
        return FrameView{fasm_xilinx_frames_address(get(), index),
                         ::fasm::Span<std::uint32_t>(fasm_xilinx_frames_words(get(), index),
                                                     words_per_frame())};
    }

    /** @brief The words of the frame at `address`, or `std::nullopt`. */
    std::optional<::fasm::Span<std::uint32_t>> find(std::uint32_t address) const noexcept {
        const std::uint32_t *words = fasm_xilinx_frames_find(get(), address);
        if (words == nullptr) {
            return std::nullopt;
        }
        return ::fasm::Span<std::uint32_t>(words, words_per_frame());
    }

    /** @brief Sets (or inserts) the frame at `address`; `words` must have `words_per_frame()` words. */
    void set(std::uint32_t address, const std::vector<std::uint32_t> &words) {
        fasm_error *err = nullptr;
        fasm_status status =
            fasm_xilinx_frames_set(handle_.get(), address, words.data(), words.size(), &err);
        ::fasm::detail::check_status(status, err);
    }

    /** @brief The `.frm` text (byte for byte `fasm2frames`'s output). */
    std::string to_frm() const {
        fasm_string *out = nullptr;
        fasm_error *err = nullptr;
        fasm_status status = fasm_xilinx_frames_to_frm(get(), &out, &err);
        ::fasm::detail::check_status(status, err);
        return ::fasm::String(out).str();
    }

    /** @brief Writes the `.frm` file `path`. */
    void write_frm(const std::filesystem::path &path) const {
        fasm_error *err = nullptr;
        std::string p = detail::path_string(path);
        fasm_status status = fasm_xilinx_frames_write_frm(get(), p.c_str(), &err);
        ::fasm::detail::check_status(status, err);
    }

    /** @brief `fasm_xilinx_frames_parse_frm`. */
    static Frames parse_frm(std::string_view text, std::size_t words_per_frame,
                            const WarningFn &warning = {}) {
        fasm_xilinx_frames *out = nullptr;
        fasm_error *err = nullptr;
        detail::WarningContext ctx{&warning, nullptr};
        fasm_status status = fasm_xilinx_frames_parse_frm(
            text.data(), text.size(), words_per_frame,
            warning ? detail::warning_trampoline : nullptr, &ctx, &out, &err);
        if (ctx.pending) {
            fasm_xilinx_frames_free(out);
        }
        detail::finish(ctx, status, err);
        return Frames::from_raw(out);
    }

    /** @brief `fasm_xilinx_frames_read_frm`. */
    static Frames read_frm(const std::filesystem::path &path, std::size_t words_per_frame,
                           const WarningFn &warning = {}) {
        fasm_xilinx_frames *out = nullptr;
        fasm_error *err = nullptr;
        detail::WarningContext ctx{&warning, nullptr};
        std::string p = detail::path_string(path);
        fasm_status status = fasm_xilinx_frames_read_frm(
            p.c_str(), words_per_frame, warning ? detail::warning_trampoline : nullptr, &ctx,
            &out, &err);
        if (ctx.pending) {
            fasm_xilinx_frames_free(out);
        }
        detail::finish(ctx, status, err);
        return Frames::from_raw(out);
    }

    friend bool operator==(const Frames &a, const Frames &b) noexcept {
        return fasm_xilinx_frames_equal(a.get(), b.get());
    }
    friend bool operator!=(const Frames &a, const Frames &b) noexcept { return !(a == b); }

    /** @brief The underlying `fasm_xilinx_frames *`, still owned by this object. */
    const fasm_xilinx_frames *get() const noexcept { return handle_.get(); }

private:
    struct Adopt {};
    Frames(Adopt, fasm_xilinx_frames *ptr) noexcept : handle_(ptr) {}

    detail::Handle<fasm_xilinx_frames, fasm_xilinx_frames_free> handle_;
};

/** @brief The flags of the `fasm2frames` tool (`fasm_xilinx_fasm2frames_options`). */
struct Fasm2FramesOptions {
    bool sparse = false;
    bool emit_pudc_b_pullup = false;
    /** @brief A ROI `design.json`. */
    std::optional<std::filesystem::path> roi;
    /** @brief Receives the warnings (bits beyond the end of a frame). */
    WarningFn warning;
};

namespace detail {

template <class Call>
inline Frames fasm2frames_with(const Fasm2FramesOptions &options, Call call) {
    fasm_xilinx_frames *out = nullptr;
    fasm_error *err = nullptr;
    WarningContext ctx{&options.warning, nullptr};
    std::string roi = options.roi ? path_string(*options.roi) : std::string();
    fasm_xilinx_fasm2frames_options c_options{};
    c_options.sparse = options.sparse;
    c_options.emit_pudc_b_pullup = options.emit_pudc_b_pullup;
    c_options.roi = options.roi ? roi.c_str() : nullptr;
    c_options.warning = options.warning ? warning_trampoline : nullptr;
    c_options.user = &ctx;
    fasm_status status = call(&c_options, &out, &err);
    if (ctx.pending) {
        fasm_xilinx_frames_free(out);
    }
    finish(ctx, status, err);
    return Frames::from_raw(out);
}

} // namespace detail

/** @brief `fasm_xilinx_fasm2frames_file`: the whole `fasm2frames` flow on a FASM file. */
inline Frames fasm2frames(const Database &db, const std::filesystem::path &fasm,
                          const Fasm2FramesOptions &options = {}) {
    std::string p = detail::path_string(fasm);
    return detail::fasm2frames_with(options, [&](const fasm_xilinx_fasm2frames_options *o,
                                                 fasm_xilinx_frames **out, fasm_error **err) {
        return fasm_xilinx_fasm2frames_file(db.get(), p.c_str(), o, out, err);
    });
}

/** @brief `fasm_xilinx_fasm2frames_string`: the whole `fasm2frames` flow on FASM text. */
inline Frames fasm2frames_string(const Database &db, std::string_view text,
                                 const Fasm2FramesOptions &options = {}) {
    return detail::fasm2frames_with(options, [&](const fasm_xilinx_fasm2frames_options *o,
                                                 fasm_xilinx_frames **out, fasm_error **err) {
        return fasm_xilinx_fasm2frames_string(db.get(), text.data(), text.size(), o, out, err);
    });
}

/**
 * @brief The FASM -> frames assembler (`fasm_xilinx_assembler`; shares
 * the ownership of its database; one thread at a time).
 */
class Assembler {
public:
    /** @brief `fasm_xilinx_assembler_new`. */
    explicit Assembler(const Database &db) {
        fasm_xilinx_assembler *out = nullptr;
        fasm_error *err = nullptr;
        fasm_status status = fasm_xilinx_assembler_new(db.get(), &out, &err);
        ::fasm::detail::check_status(status, err);
        handle_ = Handle(out);
    }

    /** @brief prjuray's (`true`) or prjxray's (`false`) semantics. */
    void set_prjuray(bool prjuray) noexcept { fasm_xilinx_assembler_set_prjuray(get(), prjuray); }

    /** @brief `fasm_xilinx_assembler_parse_file`. */
    void parse_file(const std::filesystem::path &path) {
        std::string p = detail::path_string(path);
        call([&](fasm_error **err) {
            return fasm_xilinx_assembler_parse_file(get(), p.c_str(), err);
        });
    }

    /** @brief `fasm_xilinx_assembler_parse_string`. */
    void parse_string(std::string_view text) {
        call([&](fasm_error **err) {
            return fasm_xilinx_assembler_parse_string(get(), text.data(), text.size(), err);
        });
    }

    /** @brief `fasm_xilinx_assembler_add_file`: the lines of a parsed model. */
    void add_file(const ::fasm::File &file) {
        call([&](fasm_error **err) {
            return fasm_xilinx_assembler_add_file(get(), file.raw(), err);
        });
    }

    /** @brief `fasm_xilinx_assembler_add_required_features`. */
    void add_required_features() {
        call([&](fasm_error **err) {
            return fasm_xilinx_assembler_add_required_features(get(), err);
        });
    }

    /** @brief `fasm_xilinx_assembler_mark_roi`. */
    void mark_roi(double x1, double x2, double y1, double y2) {
        call([&](fasm_error **err) {
            return fasm_xilinx_assembler_mark_roi(get(), x1, x2, y1, y2, err);
        });
    }

    /** @brief `fasm_xilinx_assembler_propagate_stepdown`. */
    void propagate_stepdown() {
        call([&](fasm_error **err) {
            return fasm_xilinx_assembler_propagate_stepdown(get(), err);
        });
    }

    /** @brief The warnings so far (views valid until this object is modified). */
    std::vector<std::string_view> warnings() const {
        std::vector<std::string_view> result;
        std::size_t n = fasm_xilinx_assembler_warning_count(handle_.get());
        for (std::size_t i = 0; i < n; ++i) {
            result.push_back(
                ::fasm::detail::to_sv(fasm_xilinx_assembler_warning(handle_.get(), i)));
        }
        return result;
    }

    /** @brief `fasm_xilinx_assembler_get_frames`. */
    Frames get_frames(bool sparse = false) const {
        fasm_xilinx_frames *out = nullptr;
        fasm_error *err = nullptr;
        fasm_status status = fasm_xilinx_assembler_get_frames(handle_.get(), sparse, &out, &err);
        ::fasm::detail::check_status(status, err);
        return Frames::from_raw(out);
    }

    /** @brief The underlying `fasm_xilinx_assembler *`, still owned by this object. */
    fasm_xilinx_assembler *get() noexcept { return handle_.get(); }

private:
    using Handle = detail::Handle<fasm_xilinx_assembler, fasm_xilinx_assembler_free>;

    template <class F>
    static void call(F f) {
        fasm_error *err = nullptr;
        fasm_status status = f(&err);
        ::fasm::detail::check_status(status, err);
    }

    Handle handle_;
};

/** @brief The frame tree and IDCODE a bitstream is written for (`fasm_xilinx_part`). */
class Part {
public:
    /** @brief `fasm_xilinx_part_from_database`. */
    static Part from_database(const Database &db) {
        fasm_xilinx_part *out = nullptr;
        fasm_error *err = nullptr;
        fasm_status status = fasm_xilinx_part_from_database(db.get(), &out, &err);
        ::fasm::detail::check_status(status, err);
        return Part(out);
    }

    /** @brief `fasm_xilinx_part_read_yaml` (an untagged file is read as `architecture`). */
    static Part read_yaml(const std::filesystem::path &path,
                          Architecture architecture = Architecture::Series7) {
        fasm_xilinx_part *out = nullptr;
        fasm_error *err = nullptr;
        std::string p = detail::path_string(path);
        fasm_status status = fasm_xilinx_part_read_yaml(
            p.c_str(), static_cast<std::int32_t>(architecture), &out, &err);
        ::fasm::detail::check_status(status, err);
        return Part(out);
    }

    /** @brief The architecture. */
    Architecture architecture() const noexcept {
        return static_cast<Architecture>(fasm_xilinx_part_architecture(get()));
    }

    /** @brief The underlying `fasm_xilinx_part *`, still owned by this object. */
    const fasm_xilinx_part *get() const noexcept { return handle_.get(); }

private:
    explicit Part(fasm_xilinx_part *ptr) noexcept : handle_(ptr) {}

    detail::Handle<fasm_xilinx_part, fasm_xilinx_part_free> handle_;
};

/** @brief The options of the bitstream writer (`fasm_xilinx_bitstream_options`). */
struct BitstreamOptions {
    BitstreamFormat format = BitstreamFormat::Default;
    /** @brief Header part name (default: the database's part). */
    std::optional<std::string> part_name;
    /** @brief Header design name (`xc7frames2bit` writes its `--frm_file`). */
    std::optional<std::string> design_name;
    /** @brief Generator (default `xc7frames2bit`). */
    std::optional<std::string> generator;
    /** @brief Header date and time (default `$SOURCE_DATE_EPOCH`, else now). */
    std::optional<std::int64_t> source_date_epoch;
};

namespace detail {

inline fasm_xilinx_bitstream_options to_c(const BitstreamOptions &o) {
    fasm_xilinx_bitstream_options c{};
    c.format = static_cast<std::int32_t>(o.format);
    c.part_name = o.part_name ? o.part_name->c_str() : nullptr;
    c.design_name = o.design_name ? o.design_name->c_str() : nullptr;
    c.generator = o.generator ? o.generator->c_str() : nullptr;
    c.has_source_date_epoch = o.source_date_epoch.has_value();
    c.source_date_epoch = o.source_date_epoch.value_or(0);
    return c;
}

} // namespace detail

/** @brief `fasm_xilinx_bitstream_write`: the `.bit` file for `frames`. */
inline std::vector<std::uint8_t> write_bitstream(const Part &part, const Frames &frames,
                                                 const BitstreamOptions &options = {}) {
    fasm_bytes *out = nullptr;
    fasm_error *err = nullptr;
    fasm_xilinx_bitstream_options c = detail::to_c(options);
    fasm_status status = fasm_xilinx_bitstream_write(part.get(), frames.get(), &c, &out, &err);
    ::fasm::detail::check_status(status, err);
    const std::uint8_t *data = fasm_bytes_data(out);
    std::size_t len = fasm_bytes_len(out);
    std::vector<std::uint8_t> result(data, data + len);
    fasm_bytes_free(out);
    return result;
}

/** @brief `fasm_xilinx_bitstream_write_file`. */
inline void write_bitstream_file(const Part &part, const Frames &frames,
                                 const std::filesystem::path &path,
                                 const BitstreamOptions &options = {}) {
    fasm_error *err = nullptr;
    fasm_xilinx_bitstream_options c = detail::to_c(options);
    std::string p = detail::path_string(path);
    fasm_status status =
        fasm_xilinx_bitstream_write_file(part.get(), frames.get(), &c, p.c_str(), &err);
    ::fasm::detail::check_status(status, err);
}

/** @brief `fasm_xilinx_bitstream_read`: the frames of a bitstream. */
inline Frames read_bitstream(const Part &part, const std::vector<std::uint8_t> &data,
                             BitstreamFormat format = BitstreamFormat::Default,
                             bool clear_ecc = true, bool skip_zero = false) {
    fasm_xilinx_frames *out = nullptr;
    fasm_error *err = nullptr;
    fasm_status status = fasm_xilinx_bitstream_read(part.get(), data.data(), data.size(),
                                                    static_cast<std::int32_t>(format), clear_ecc,
                                                    skip_zero, &out, &err);
    ::fasm::detail::check_status(status, err);
    return Frames::from_raw(out);
}

/** @brief `fasm_xilinx_bitstream_read_file`. */
inline Frames read_bitstream_file(const Part &part, const std::filesystem::path &path,
                                  BitstreamFormat format = BitstreamFormat::Default,
                                  bool clear_ecc = true, bool skip_zero = false) {
    fasm_xilinx_frames *out = nullptr;
    fasm_error *err = nullptr;
    std::string p = detail::path_string(path);
    fasm_status status =
        fasm_xilinx_bitstream_read_file(part.get(), p.c_str(), static_cast<std::int32_t>(format),
                                        clear_ecc, skip_zero, &out, &err);
    ::fasm::detail::check_status(status, err);
    return Frames::from_raw(out);
}

} // namespace fasm::xilinx

#endif // FASM_HPP_INCLUDED
