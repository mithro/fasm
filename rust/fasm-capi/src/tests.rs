// Copyright 2017-2022 F4PGA Authors
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
//
// SPDX-License-Identifier: Apache-2.0

//! Tests calling the C ABI functions from Rust (the C side is covered by
//! `tests/c/test_capi.c`).

use std::ffi::{c_char, c_void, CStr, CString};
use std::ptr;

use crate::error::CapiError;
use crate::ffi::{catch, run, run_status};
use crate::*;

/// Path of a file relative to the repository root.
fn repo_path(rel: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel)
}

/// Copies an owned `fasm_string` into a `String` and frees it.
fn take_string(s: *mut fasm_string) -> String {
    assert!(!s.is_null());
    // SAFETY: `s` is a live string from the library, freed once here.
    unsafe {
        let len = fasm_string_len(s);
        let data = fasm_string_data(s);
        let bytes = std::slice::from_raw_parts(data.cast::<u8>(), len);
        assert_eq!(*data.add(len), 0, "fasm_string is NUL terminated");
        let out = String::from_utf8(bytes.to_vec()).unwrap();
        fasm_string_free(s);
        out
    }
}

/// The text of a borrowed `fasm_str`.
fn view(s: fasm_str) -> String {
    if s.len == 0 {
        return String::new();
    }
    // SAFETY: filled in by the library from a live object.
    let bytes = unsafe { std::slice::from_raw_parts(s.ptr.cast::<u8>(), s.len) };
    String::from_utf8(bytes.to_vec()).unwrap()
}

/// Parses `text`, panicking on error.
fn parse(text: &str) -> *mut fasm_file {
    let mut file = ptr::null_mut();
    let mut err = ptr::null_mut();
    // SAFETY: valid pointers.
    let status =
        unsafe { fasm_parse_string(text.as_ptr().cast(), text.len(), &mut file, &mut err) };
    assert_eq!(status, fasm_status::FASM_OK);
    assert!(err.is_null());
    assert!(!file.is_null());
    file
}

/// Parses `text`, expecting an error; returns (status, line, column,
/// message).
fn parse_err(text: &[u8]) -> (fasm_status, usize, usize, String) {
    let mut file = ptr::null_mut();
    let mut err = ptr::null_mut();
    // SAFETY: valid pointers.
    unsafe {
        let status = fasm_parse_string(text.as_ptr().cast(), text.len(), &mut file, &mut err);
        assert!(file.is_null());
        assert!(!err.is_null());
        assert_eq!(fasm_error_status(err), status);
        let message = CStr::from_ptr(fasm_error_message(err))
            .to_str()
            .unwrap()
            .to_owned();
        let out = (
            status,
            fasm_error_line(err),
            fasm_error_column(err),
            message,
        );
        fasm_error_free(err);
        out
    }
}

/// The feature name of `sf` via the buffer API.
fn name(sf: *const fasm_set_feature) -> String {
    // SAFETY: `sf` is valid; the buffer is large enough.
    unsafe {
        let len = fasm_set_feature_name_len(sf);
        let mut buf = vec![0 as c_char; len + 1];
        assert_eq!(fasm_set_feature_name(sf, buf.as_mut_ptr(), buf.len()), len);
        CStr::from_ptr(buf.as_ptr()).to_str().unwrap().to_owned()
    }
}

#[test]
fn version() {
    // SAFETY: static NUL terminated string.
    let v = unsafe { CStr::from_ptr(fasm_version()) };
    assert_eq!(v.to_str().unwrap(), env!("CARGO_PKG_VERSION"));
}

#[test]
fn status_strings() {
    for (status, text) in [
        (0, "ok"),
        (1, "parse error"),
        (2, "I/O error"),
        (3, "invalid argument"),
        (4, "invalid UTF-8"),
        (5, "internal error (panic)"),
        (6, "output error"),
        (7, "unknown status"),
        (-1, "unknown status"),
    ] {
        // SAFETY: static NUL terminated string.
        let s = unsafe { CStr::from_ptr(fasm_status_string(status)) };
        assert_eq!(s.to_str().unwrap(), text);
    }
}

#[test]
fn parse_and_access_many_fasm() {
    let path = CString::new(repo_path("examples/many.fasm").to_str().unwrap()).unwrap();
    let mut file = ptr::null_mut();
    // SAFETY: valid pointers; `err` may be NULL.
    unsafe {
        assert_eq!(
            fasm_parse_file(path.as_ptr(), &mut file, ptr::null_mut()),
            fasm_status::FASM_OK
        );
        assert_eq!(fasm_file_line_count(file), 40);
        assert!(fasm_file_line(file, 40).is_null());

        // Line 0: a comment only.
        let line = fasm_file_line(file, 0);
        assert!(!fasm_line_has_set_feature(line));
        assert!(fasm_line_set_feature(line).is_null());
        assert!(fasm_line_has_comment(line));
        let mut comment = fasm_str::empty();
        assert!(fasm_line_comment(line, &mut comment));
        assert_eq!(
            view(comment),
            " This file should have examples of all FASM lines that should parse."
        );
        assert_eq!(fasm_line_annotation_count(line), 0);

        // Line 3: a bare `#`.
        let line = fasm_file_line(file, 3);
        assert!(fasm_line_comment(line, &mut comment));
        assert_eq!(comment.len, 0);

        // Line 10: `INT_L_X10Y146.SW6BEG0.WW2END0` (implicit 1).
        let line = fasm_file_line(file, 10);
        assert!(!fasm_line_has_comment(line));
        assert!(!fasm_line_comment(line, &mut comment));
        assert!(comment.ptr.is_null());
        let sf = fasm_line_set_feature(line);
        assert_eq!(name(sf), "INT_L_X10Y146.SW6BEG0.WW2END0");
        assert!(!fasm_set_feature_has_start(sf));
        assert!(!fasm_set_feature_has_end(sf));
        assert_eq!(
            fasm_set_feature_value_format(sf),
            fasm_value_format::FASM_VALUE_FORMAT_NONE
        );
        assert_eq!(fasm_set_feature_width(sf), 1);
        let mut v = 0;
        assert!(fasm_set_feature_value_u64(sf, &mut v));
        assert_eq!(v, 1);

        // Line 26: `CLBLL_R_X13Y132.SLICEL_X0.ALUT.INIT[63:32] = 32'b...`.
        let sf = fasm_line_set_feature(fasm_file_line(file, 26));
        assert!(fasm_set_feature_has_start(sf));
        assert_eq!(fasm_set_feature_start(sf), 32);
        assert!(fasm_set_feature_has_end(sf));
        assert_eq!(fasm_set_feature_end(sf), 63);
        assert_eq!(fasm_set_feature_width(sf), 32);
        assert_eq!(
            fasm_set_feature_value_format(sf),
            fasm_value_format::FASM_VALUE_FORMAT_VERILOG_BINARY
        );
        assert!(fasm_set_feature_value_u64(sf, &mut v));
        assert_eq!(v, 4_042_322_160);
        assert_eq!(fasm_set_feature_value_bits(sf), 32);
        assert!(fasm_set_feature_value_bit(sf, 31));
        assert!(!fasm_set_feature_value_bit(sf, 0));
        assert!(!fasm_set_feature_value_bit(sf, 1000));
        let mut bytes = [0xAAu8; 6];
        assert_eq!(fasm_set_feature_value_bytes_le(sf, ptr::null_mut(), 0), 4);
        assert_eq!(
            fasm_set_feature_value_bytes_le(sf, bytes.as_mut_ptr(), 3),
            4
        );
        assert_eq!(bytes, [0xAA; 6], "too small a buffer is not touched");
        assert_eq!(
            fasm_set_feature_value_bytes_le(sf, bytes.as_mut_ptr(), 6),
            4
        );
        assert_eq!(bytes, [0xF0, 0xF0, 0xF0, 0xF0, 0, 0]);
        let hex = fasm_set_feature_value_to_string(sf, 16, true, ptr::null_mut());
        assert_eq!(take_string(hex), "F0F0F0F0");
        let hex = fasm_set_feature_value_to_string(sf, 16, false, ptr::null_mut());
        assert_eq!(take_string(hex), "f0f0f0f0");
        let mut err = ptr::null_mut();
        assert!(fasm_set_feature_value_to_string(sf, 3, false, &mut err).is_null());
        assert_eq!(fasm_error_status(err), fasm_status::FASM_ERR_INVALID_ARG);
        fasm_error_free(err);
        assert_eq!(
            take_string(fasm_set_feature_to_string(sf, false, ptr::null_mut())),
            "CLBLL_R_X13Y132.SLICEL_X0.ALUT.INIT[63:32] = 32'b11110000111100001111000011110000"
        );
        assert!(fasm_set_feature_to_string(sf, true, &mut err).is_null());
        assert_eq!(fasm_error_status(err), fasm_status::FASM_ERR_OUTPUT);
        fasm_error_free(err);
        assert_eq!(
            take_string(fasm_set_feature_name_string(sf)),
            "CLBLL_R_X13Y132.SLICEL_X0.ALUT.INIT"
        );

        // Line 33: three annotations.
        let line = fasm_file_line(file, 33);
        assert_eq!(fasm_line_annotation_count(line), 3);
        let mut a = fasm_annotation {
            name: fasm_str::empty(),
            value: fasm_str::empty(),
        };
        let expected = [
            ("module", "top"),
            ("file", "/a/b/d.txt"),
            ("line_number", "123"),
        ];
        for (i, (n, v)) in expected.iter().enumerate() {
            assert!(fasm_line_annotation(line, i, &mut a));
            assert_eq!((view(a.name).as_str(), view(a.value).as_str()), (*n, *v));
        }
        assert!(!fasm_line_annotation(line, 3, &mut a));
        assert!(!fasm_line_annotation(line, 0, ptr::null_mut()));
        assert_eq!(
            take_string(fasm_line_to_string(line, false, ptr::null_mut())),
            "INT_L_X10Y146.SW6BEG0.WW2END0 { module = \"top\", file = \"/a/b/d.txt\", \
             line_number = \"123\" }"
        );
        assert_eq!(
            take_string(fasm_line_to_string(line, true, ptr::null_mut())),
            "INT_L_X10Y146.SW6BEG0.WW2END0"
        );

        // Whole file output matches the Python oracle.
        let out = take_string(fasm_file_to_string(file, false, ptr::null_mut()));
        let expected = std::fs::read_to_string(repo_path("tests/corpus/oracle/many.fasm.out.txt"));
        assert_eq!(out, expected.unwrap());
        let out = take_string(fasm_file_to_string(file, true, ptr::null_mut()));
        let expected =
            std::fs::read_to_string(repo_path("tests/corpus/oracle/many.fasm.canonical.txt"));
        assert_eq!(out, expected.unwrap());

        fasm_file_free(file);
    }
}

#[test]
fn feature_name_buffer_truncates_like_snprintf() {
    let file = parse("ABC.DEF.GHI.JKL\n");
    // SAFETY: valid pointers and buffer sizes.
    unsafe {
        let sf = fasm_line_set_feature(fasm_file_line(file, 0));
        assert_eq!(fasm_set_feature_name(sf, ptr::null_mut(), 0), 15);
        let mut buf = [b'x' as c_char; 6];
        assert_eq!(fasm_set_feature_name(sf, buf.as_mut_ptr(), buf.len()), 15);
        assert_eq!(CStr::from_ptr(buf.as_ptr()).to_str().unwrap(), "ABC.D");
        assert_eq!(fasm_set_feature_name(sf, buf.as_mut_ptr(), 1), 15);
        assert_eq!(buf[0], 0);
        assert_eq!(fasm_set_feature_name(ptr::null(), buf.as_mut_ptr(), 6), 0);
        assert_eq!(buf[0], 0);
        fasm_file_free(file);
    }
}

#[test]
fn wide_value_bytes() {
    let text = format!("A.B[255:0] = 256'h8{}1\n", "0".repeat(62));
    let file = parse(&text);
    // SAFETY: valid pointers and buffer sizes.
    unsafe {
        let sf = fasm_line_set_feature(fasm_file_line(file, 0));
        assert_eq!(fasm_set_feature_value_bits(sf), 256);
        let mut v = 0;
        assert!(!fasm_set_feature_value_u64(sf, &mut v));
        let mut buf = [0u8; 32];
        assert_eq!(
            fasm_set_feature_value_bytes_le(sf, buf.as_mut_ptr(), 32),
            32
        );
        let mut expected = [0u8; 32];
        expected[0] = 1;
        expected[31] = 0x80;
        assert_eq!(buf, expected);
        assert!(fasm_set_feature_value_bit(sf, 255));
        assert!(fasm_set_feature_value_bit(sf, 0));
        assert!(!fasm_set_feature_value_bit(sf, 254));
        fasm_file_free(file);
    }
}

#[test]
fn parse_errors() {
    let (status, line, column, message) = parse_err(b"A.B\nA.B[3:0] = 5'h1F\n");
    assert_eq!(status, fasm_status::FASM_ERR_PARSE);
    assert_eq!((line, column), (2, 11));
    assert!(message.starts_with("Parse error at 2:11 - "), "{message}");

    let (status, line, _, _) = parse_err(b"A.B\n# \xff\n");
    assert_eq!(status, fasm_status::FASM_ERR_UTF8);
    assert_eq!(line, 2);

    // NULL with a length, NULL out.
    let mut err = ptr::null_mut();
    // SAFETY: invalid arguments are rejected before any access.
    unsafe {
        let mut file = ptr::null_mut();
        let status = fasm_parse_string(ptr::null(), 3, &mut file, &mut err);
        assert_eq!(status, fasm_status::FASM_ERR_INVALID_ARG);
        assert_eq!(fasm_error_status(err), status);
        fasm_error_free(err);
        let status = fasm_parse_string(c"A".as_ptr(), 1, ptr::null_mut(), ptr::null_mut());
        assert_eq!(status, fasm_status::FASM_ERR_INVALID_ARG);
        // NULL with 0 is an empty file.
        assert_eq!(
            fasm_parse_string(ptr::null(), 0, &mut file, ptr::null_mut()),
            fasm_status::FASM_OK
        );
        assert_eq!(fasm_file_line_count(file), 0);
        assert_eq!(
            take_string(fasm_file_to_string(file, false, ptr::null_mut())),
            "\n"
        );
        fasm_file_free(file);
    }
}

#[test]
fn parse_file_errors() {
    let mut file = ptr::null_mut();
    let mut err = ptr::null_mut();
    // SAFETY: valid pointers.
    unsafe {
        let status = fasm_parse_file(c"/nonexistent/x.fasm".as_ptr(), &mut file, &mut err);
        assert_eq!(status, fasm_status::FASM_ERR_IO);
        assert!(file.is_null());
        assert_eq!((fasm_error_line(err), fasm_error_column(err)), (0, 0));
        let message = CStr::from_ptr(fasm_error_message(err)).to_str().unwrap();
        assert!(message.contains("/nonexistent/x.fasm"), "{message}");
        fasm_error_free(err);

        let status = fasm_parse_file(ptr::null(), &mut file, &mut err);
        assert_eq!(status, fasm_status::FASM_ERR_INVALID_ARG);
        fasm_error_free(err);
    }
}

/// Streaming callback state.
struct Collected {
    names: Vec<(usize, String)>,
    stop_after: usize,
}

unsafe extern "C" fn collect(
    line: *const fasm_line,
    line_number: usize,
    user: *mut c_void,
) -> bool {
    // SAFETY: `user` is the `Collected` passed below; `line` is valid.
    unsafe {
        let state = &mut *user.cast::<Collected>();
        let sf = fasm_line_set_feature(line);
        let text = if sf.is_null() {
            String::new()
        } else {
            name(sf)
        };
        state.names.push((line_number, text));
        state.names.len() < state.stop_after
    }
}

#[test]
fn streaming() {
    let text = "A.B\n\n# c\nC.D[1:0] = 2'b10\nE.F\n";
    let mut state = Collected {
        names: Vec::new(),
        stop_after: usize::MAX,
    };
    let user = ptr::from_mut(&mut state).cast::<c_void>();
    // SAFETY: valid pointers; `collect` accepts `user`.
    unsafe {
        let status = fasm_parse_string_cb(
            text.as_ptr().cast(),
            text.len(),
            Some(collect),
            user,
            ptr::null_mut(),
        );
        assert_eq!(status, fasm_status::FASM_OK);
        assert_eq!(
            state.names,
            [
                (1, "A.B".to_owned()),
                (3, String::new()),
                (4, "C.D".to_owned()),
                (5, "E.F".to_owned())
            ]
        );

        // Early stop.
        state.names.clear();
        state.stop_after = 2;
        let status = fasm_parse_string_cb(
            text.as_ptr().cast(),
            text.len(),
            Some(collect),
            user,
            ptr::null_mut(),
        );
        assert_eq!(status, fasm_status::FASM_OK);
        assert_eq!(state.names.len(), 2);

        // Lines before an error are delivered.
        state.names.clear();
        state.stop_after = usize::MAX;
        let bad = "A.B\nC.D[\n";
        let mut err = ptr::null_mut();
        let status = fasm_parse_string_cb(
            bad.as_ptr().cast(),
            bad.len(),
            Some(collect),
            user,
            &mut err,
        );
        assert_eq!(status, fasm_status::FASM_ERR_PARSE);
        assert_eq!(fasm_error_line(err), 2);
        fasm_error_free(err);
        assert_eq!(state.names, [(1, "A.B".to_owned())]);

        // NULL callback.
        let status = fasm_parse_string_cb(text.as_ptr().cast(), text.len(), None, user, &mut err);
        assert_eq!(status, fasm_status::FASM_ERR_INVALID_ARG);
        fasm_error_free(err);

        // From a file.
        state.names.clear();
        let path = CString::new(repo_path("examples/many.fasm").to_str().unwrap()).unwrap();
        let status = fasm_parse_file_cb(path.as_ptr(), Some(collect), user, ptr::null_mut());
        assert_eq!(status, fasm_status::FASM_OK);
        assert_eq!(state.names.len(), 40);
        assert_eq!(
            state.names[10],
            (13, "INT_L_X10Y146.SW6BEG0.WW2END0".to_owned())
        );
        let status = fasm_parse_file_cb(c"/nonexistent".as_ptr(), Some(collect), user, &mut err);
        assert_eq!(status, fasm_status::FASM_ERR_IO);
        fasm_error_free(err);
    }
}

/// A `fasm_str` view of `s`.
fn s(s: &str) -> fasm_str {
    fasm_str::from_str(s)
}

/// A spec without address and value.
fn spec(name: &str) -> fasm_set_feature_spec {
    fasm_set_feature_spec {
        feature: s(name),
        has_start: false,
        start: 0,
        has_end: false,
        end: 0,
        value_le: ptr::null(),
        value_len: 0,
        value_format: -1,
    }
}

#[test]
fn build_and_print() {
    // SAFETY: valid pointers.
    unsafe {
        let file = fasm_file_new();
        let mut err = ptr::null_mut();

        let mut f = spec("X.Y.INIT");
        let value = [0x2Au8, 0x01];
        f.has_start = true;
        f.has_end = true;
        f.end = 15;
        f.value_le = value.as_ptr();
        f.value_len = value.len();
        f.value_format = fasm_value_format::FASM_VALUE_FORMAT_VERILOG_HEX as i32;
        let annotations = [fasm_annotation {
            name: s("a"),
            value: s("b c"),
        }];
        let comment = s(" note");
        let status = fasm_file_push_line(file, &f, annotations.as_ptr(), 1, &comment, &mut err);
        assert_eq!(status, fasm_status::FASM_OK);
        assert!(err.is_null());

        let mut one = spec("X.Y.EN");
        let value = [1u8];
        one.value_le = value.as_ptr();
        one.value_len = 1;
        assert_eq!(
            fasm_file_push_line(file, &one, ptr::null(), 0, ptr::null(), &mut err),
            fasm_status::FASM_OK
        );
        let empty = s("");
        assert_eq!(
            fasm_file_push_line(file, ptr::null(), ptr::null(), 0, &empty, &mut err),
            fasm_status::FASM_OK
        );
        assert_eq!(
            fasm_file_push_line(file, ptr::null(), ptr::null(), 0, ptr::null(), &mut err),
            fasm_status::FASM_OK
        );

        assert_eq!(fasm_file_line_count(file), 4);
        assert_eq!(
            take_string(fasm_file_to_string(file, false, ptr::null_mut())),
            "X.Y.INIT[15:0] = 16'h12A { a = \"b c\" } # note\nX.Y.EN\n#\n\n"
        );

        // Validation errors append nothing.
        let mut bad = spec("X.Y.Z");
        bad.has_end = true;
        bad.end = 3;
        let status = fasm_file_push_line(file, &bad, ptr::null(), 0, ptr::null(), &mut err);
        assert_eq!(status, fasm_status::FASM_ERR_INVALID_ARG);
        fasm_error_free(err);
        let mut bad = spec("X.Y.Z");
        let value = [2u8];
        bad.value_le = value.as_ptr();
        bad.value_len = 1;
        let status = fasm_file_push_line(file, &bad, ptr::null(), 0, ptr::null(), &mut err);
        assert_eq!(status, fasm_status::FASM_ERR_INVALID_ARG);
        let message = CStr::from_ptr(fasm_error_message(err)).to_str().unwrap();
        assert!(message.contains("does not fit"), "{message}");
        fasm_error_free(err);
        let mut bad = spec("X.Y.Z");
        bad.value_format = 5;
        let status = fasm_file_push_line(file, &bad, ptr::null(), 0, ptr::null(), &mut err);
        assert_eq!(status, fasm_status::FASM_ERR_INVALID_ARG);
        fasm_error_free(err);
        let bad = fasm_set_feature_spec {
            feature: fasm_str {
                ptr: b"\xff".as_ptr().cast(),
                len: 1,
            },
            ..spec("")
        };
        let status = fasm_file_push_line(file, &bad, ptr::null(), 0, ptr::null(), &mut err);
        assert_eq!(status, fasm_status::FASM_ERR_UTF8);
        fasm_error_free(err);
        let status = fasm_file_push_line(file, ptr::null(), ptr::null(), 2, ptr::null(), &mut err);
        assert_eq!(status, fasm_status::FASM_ERR_INVALID_ARG);
        fasm_error_free(err);
        let bad_comment = fasm_str {
            ptr: ptr::null(),
            len: 4,
        };
        let status = fasm_file_push_line(file, ptr::null(), ptr::null(), 0, &bad_comment, &mut err);
        assert_eq!(status, fasm_status::FASM_ERR_INVALID_ARG);
        fasm_error_free(err);
        let status = fasm_file_push_line(
            ptr::null_mut(),
            ptr::null(),
            ptr::null(),
            0,
            ptr::null(),
            &mut err,
        );
        assert_eq!(status, fasm_status::FASM_ERR_INVALID_ARG);
        fasm_error_free(err);
        assert_eq!(fasm_file_line_count(file), 4);

        fasm_file_free(file);
    }
}

unsafe extern "C" fn zero_if_contains_zero(
    name: *const c_char,
    len: usize,
    _: *mut c_void,
) -> bool {
    // SAFETY: NUL terminated, `len` bytes.
    let name = unsafe { CStr::from_ptr(name) }.to_str().unwrap();
    assert_eq!(name.len(), len);
    name.contains("ZERO")
}

unsafe extern "C" fn reverse_key(group: *const c_char, _: usize, _: *mut c_void) -> i64 {
    // SAFETY: NUL terminated.
    let group = unsafe { CStr::from_ptr(group) }.to_bytes();
    -i64::from(group[0])
}

#[test]
fn merge_and_sort() {
    let file = parse("B.X[1]\n# about A\nA.Y\nB.X[0]\nC.ZERO\n");
    // SAFETY: valid pointers; the callbacks ignore `user`.
    unsafe {
        let merged = fasm_file_merge_and_sort(file, ptr::null_mut());
        assert_eq!(
            take_string(fasm_file_to_string(merged, false, ptr::null_mut())),
            "# about A\nA.Y\n\nB.X[1:0] = 2'b11\n\nC.ZERO\n"
        );
        fasm_file_free(merged);

        let merged = fasm_file_merge_and_sort_ex(
            file,
            Some(zero_if_contains_zero),
            Some(reverse_key),
            ptr::null_mut(),
            ptr::null_mut(),
        );
        assert_eq!(
            take_string(fasm_file_to_string(merged, false, ptr::null_mut())),
            "B.X[1:0] = 2'b11\n\n# about A\nA.Y\n"
        );
        fasm_file_free(merged);

        let mut err = ptr::null_mut();
        assert!(fasm_file_merge_and_sort(ptr::null(), &mut err).is_null());
        assert_eq!(fasm_error_status(err), fasm_status::FASM_ERR_INVALID_ARG);
        fasm_error_free(err);
        fasm_file_free(file);
    }
}

#[test]
fn null_handles_are_harmless() {
    // SAFETY: every function accepts NULL handles.
    unsafe {
        assert_eq!(fasm_file_line_count(ptr::null()), 0);
        assert!(fasm_file_line(ptr::null(), 0).is_null());
        fasm_file_free(ptr::null_mut());
        assert!(!fasm_line_has_set_feature(ptr::null()));
        assert!(fasm_line_set_feature(ptr::null()).is_null());
        assert_eq!(fasm_line_annotation_count(ptr::null()), 0);
        assert!(!fasm_line_has_comment(ptr::null()));
        assert!(!fasm_line_comment(ptr::null(), ptr::null_mut()));
        let sf = ptr::null();
        assert_eq!(fasm_set_feature_name_len(sf), 0);
        assert!(fasm_set_feature_name_string(sf).is_null());
        assert!(!fasm_set_feature_has_start(sf));
        assert_eq!(fasm_set_feature_start(sf), 0);
        assert!(!fasm_set_feature_has_end(sf));
        assert_eq!(fasm_set_feature_end(sf), 0);
        assert_eq!(
            fasm_set_feature_value_format(sf),
            fasm_value_format::FASM_VALUE_FORMAT_NONE
        );
        assert_eq!(fasm_set_feature_width(sf), 0);
        assert_eq!(fasm_set_feature_value_bits(sf), 0);
        assert!(!fasm_set_feature_value_u64(sf, ptr::null_mut()));
        assert!(!fasm_set_feature_value_bit(sf, 0));
        assert_eq!(fasm_set_feature_value_bytes_le(sf, ptr::null_mut(), 0), 0);
        assert!(fasm_set_feature_value_to_string(sf, 10, false, ptr::null_mut()).is_null());
        assert!(fasm_set_feature_to_string(sf, false, ptr::null_mut()).is_null());
        assert!(fasm_line_to_string(ptr::null(), false, ptr::null_mut()).is_null());
        assert!(fasm_file_to_string(ptr::null(), false, ptr::null_mut()).is_null());
        assert_eq!(fasm_string_len(ptr::null()), 0);
        assert_eq!(*fasm_string_data(ptr::null()), 0);
        fasm_string_free(ptr::null_mut());
        assert_eq!(
            fasm_error_status(ptr::null()),
            fasm_status::FASM_ERR_INVALID_ARG
        );
        assert_eq!(*fasm_error_message(ptr::null()), 0);
        assert_eq!(fasm_error_line(ptr::null()), 0);
        assert_eq!(fasm_error_column(ptr::null()), 0);
        fasm_error_free(ptr::null_mut());
    }
}

#[test]
fn panics_become_errors() {
    let result: Result<(), CapiError> = catch(|| panic!("boom"));
    let error = result.unwrap_err();
    assert_eq!(error.status, fasm_status::FASM_ERR_PANIC);

    let mut err = ptr::null_mut();
    // SAFETY: `err` is writable.
    unsafe {
        let value = run(&mut err, 7, || -> Result<i32, CapiError> { panic!("bang") });
        assert_eq!(value, 7);
        assert_eq!(fasm_error_status(err), fasm_status::FASM_ERR_PANIC);
        let message = CStr::from_ptr(fasm_error_message(err)).to_str().unwrap();
        assert!(message.contains("bang"), "{message}");
        fasm_error_free(err);

        let status = run_status(&mut err, || panic!("{}", String::from("owned")));
        assert_eq!(status, fasm_status::FASM_ERR_PANIC);
        let message = CStr::from_ptr(fasm_error_message(err)).to_str().unwrap();
        assert!(message.contains("owned"), "{message}");
        fasm_error_free(err);

        // Success resets `*err` to NULL.
        err = ptr::dangling_mut();
        assert_eq!(run_status(&mut err, || Ok(())), fasm_status::FASM_OK);
        assert!(err.is_null());
    }
}

#[test]
fn error_messages_drop_nul_bytes() {
    let error = CapiError::new(fasm_status::FASM_ERR_PARSE, "a\0b");
    let mut err = ptr::null_mut();
    // SAFETY: `err` is writable.
    unsafe {
        crate::error::set_error(&mut err, Some(error));
        assert_eq!(
            CStr::from_ptr(fasm_error_message(err)).to_str().unwrap(),
            "ab"
        );
        fasm_error_free(err);
    }
}
