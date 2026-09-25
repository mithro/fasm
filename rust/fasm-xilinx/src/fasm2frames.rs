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

//! [`fasm2frames`]: the whole FASM -> frames flow of
//! `xc_fasm.fasm2frames.fasm2frames` (f4pga-xc-fasm): ROI, required
//! features, PUDC_B pullup, STEPDOWN propagation over IO banks.

use std::cell::Cell;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use fasm::idstring::IdString;
use fasm::FasmLine;
use foldhash::{HashSet, HashSetExt};
use serde_json::Value;

use crate::assembler::{py_repr, AssemblerError, FasmAssembler, Roi};
use crate::db::Database;
use crate::frames::Frames;

/// The options of [`fasm2frames`] (the flags of the `fasm2frames` tool).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Fasm2FramesOptions {
    /// `--sparse`: only output the frames of the buses that were written
    /// (and of the ROI tiles), instead of every frame of the part.
    pub sparse: bool,
    /// `--roi design.json`: mark every frame of the tiles inside the ROI
    /// in use and add its `required_features`. An empty path counts as
    /// no ROI (Python truthiness).
    pub roi: Option<PathBuf>,
    /// `--emit_pudc_b_pullup`: enable an input with a pullup on the
    /// PUDC_B pin if the FASM does not use its IOB site.
    pub emit_pudc_b_pullup: bool,
}

/// The features that make the PUDC_B IOB an input with a pullup
/// (`xc_fasm/fasm2frames.py`: "only works on Artix 50T and Zynq 10
/// fabrics").
const PUDC_B_FEATURES: [&str; 3] = [
    "LVCMOS12_LVCMOS15_LVCMOS18_LVCMOS25_LVCMOS33_LVDS_25_LVTTL_SSTL135_SSTL15_TMDS_33.IN_ONLY",
    "LVCMOS25_LVCMOS33_LVTTL.IN",
    "PULLTYPE.PULLUP",
];

/// `xc_fasm.fasm2frames.fasm2frames(db_root, part, filename_in, ...)`
/// for the part `db` was opened for: assembles the FASM file at `fasm`
/// into frames. Warnings the reference prints to stderr (bits dropped
/// beyond the end of a frame) are passed to `warn`, in order, before the
/// function returns (also when it fails).
///
/// The steps, in the reference's order:
///
/// 1. the IO bank maps: `package_pins.csv` and `part.json` `iobanks` of
///    the part must exist;
/// 2. `--emit_pudc_b_pullup`: [`find_pudc_b`], and a feature callback
///    noting whether the FASM uses the PUDC_B IOB site;
/// 3. `--roi`: [`read_roi_design`], [`FasmAssembler::mark_roi_frames`],
///    its `required_features`;
/// 4. the part's `required_features.fasm`;
/// 5. [`FasmAssembler::parse_fasm_filename`] with the extra features of 3
///    and 4 after the file's lines;
/// 6. the PUDC_B pullup features (if requested, found and unused);
/// 7. STEPDOWN propagation: if a used IOB of a bank sets a feature whose
///    tag contains `STEPDOWN`, every unused IOB site of the bank gets the
///    same tag(s) and the bank's `HCLK_IOI3` tile gets `STEPDOWN`;
/// 8. [`FasmAssembler::get_frames`].
///
/// Python iterates sets in 7 (in an order that changes from run to run);
/// here banks, tags and tiles are taken in first seen order, which only
/// matters for the order of error messages.
///
/// # Errors
///
/// See [`AssemblerError`].
pub fn fasm2frames(
    db: &Database,
    fasm: &Path,
    options: &Fasm2FramesOptions,
    warn: &mut dyn FnMut(&str),
) -> Result<Frames, AssemblerError> {
    let mut assembler = FasmAssembler::new(db)?;
    let result = run(db, &mut assembler, fasm, options);
    for warning in assembler.take_warnings() {
        warn(&warning);
    }
    result
}

/// `run(db_root, part, filename_in, ...)` of prjuray's
/// `utils/fasm2frames.py` for the part `db` was opened for: like
/// [`fasm2frames`] without the IO bank maps, the PUDC_B pullup and the
/// STEPDOWN propagation (steps 3, 4, 5 and 8), with the assembler in
/// prjuray mode ([`FasmAssembler::set_prjuray`]). `options.emit_pudc_b_pullup`
/// is ignored. The frames are 32-bit words; prjuray writes them as 16-bit
/// words ([`write_frm_halfwords`], [`dump_frames_sparse_halfwords`],
/// [`write_bits`]).
///
/// # Errors
///
/// See [`AssemblerError`].
pub fn uray_fasm2frames(
    db: &Database,
    fasm: &Path,
    options: &Fasm2FramesOptions,
) -> Result<Frames, AssemblerError> {
    let mut assembler = FasmAssembler::new(db)?;
    assembler.set_prjuray(true);
    let info = db.part_info().ok_or_else(|| AssemblerError::Python {
        exception: "AttributeError",
        message: "the database was opened without a part".to_owned(),
    })?;
    let mut extra_features: Vec<FasmLine> = Vec::new();
    if let Some(roi_path) = options.roi.as_deref().filter(|p| !p.as_os_str().is_empty()) {
        let design = read_roi_design(roi_path)?;
        assembler.mark_roi_frames(&design.roi);
        if let Some(text) = design.required_features {
            extra_features = fasm::parse_fasm_string(&text)?;
        }
    }
    let required = db.get_required_fasm_features(Some(&info.name)).join("\n");
    extra_features.extend(fasm::parse_fasm_string(&required)?);
    assembler.parse_fasm_filename(fasm, extra_features)?;
    assembler.get_frames(options.sparse)
}

/// The 16-bit words of a frame of 32-bit words, low half first (prjuray's
/// frames of `2 * words_per_frame` 16-bit words).
fn halfwords(words: &[u32]) -> impl Iterator<Item = u32> + '_ {
    words.iter().flat_map(|&w| [w & 0xFFFF, w >> 16])
}

/// `dump_frm(f, frames)` of prjuray's `utils/fasm2frames.py`: like
/// [`Frames::write_frm`], with the 16-bit words (186 for UltraScale+) as
/// `0x%08X`.
///
/// # Errors
///
/// The errors of `out`.
pub fn write_frm_halfwords(frames: &Frames, out: &mut dyn Write) -> io::Result<()> {
    let mut line = String::with_capacity(11 + 22 * frames.words_per_frame());
    for (address, words) in frames.iter() {
        line.clear();
        line.push_str(&format!("0x{address:08X} "));
        for (i, half) in halfwords(words).enumerate() {
            if i > 0 {
                line.push(',');
            }
            line.push_str(&format!("0x{half:08X}"));
        }
        line.push('\n');
        out.write_all(line.as_bytes())?;
    }
    Ok(())
}

/// `dump_frames_sparse(frames)` of prjuray's `utils/fasm2frames.py` (the
/// `--debug` output): [`dump_frames_sparse`] on the 16-bit words.
///
/// # Errors
///
/// The errors of `out`.
pub fn dump_frames_sparse_halfwords(frames: &Frames, out: &mut dyn Write) -> io::Result<()> {
    writeln!(out)?;
    writeln!(out, "Frames: {}", frames.len())?;
    for (address, words) in frames.iter() {
        if words.iter().all(|&w| w == 0) {
            continue;
        }
        writeln!(out, "Frame @ 0x{address:08X}")?;
        for (i, half) in halfwords(words).enumerate() {
            if half != 0 {
                writeln!(out, "  {:>3}: 0x{half:08X}", format!(" {i}"))?;
            }
        }
    }
    Ok(())
}

/// `output_bits(f, frames)` of prjuray's `utils/fasm2frames.py`
/// (`--dump_bits`): `bit_%08x_%03d_%02d` (frame, 32-bit word, bit) for
/// every set bit, in frame, word and bit order.
///
/// # Errors
///
/// The errors of `out`.
pub fn write_bits(frames: &Frames, out: &mut dyn Write) -> io::Result<()> {
    let mut text = String::new();
    for (address, words) in frames.iter() {
        text.clear();
        for (i, &word) in words.iter().enumerate() {
            let mut bits = word;
            while bits != 0 {
                let k = bits.trailing_zeros();
                bits &= bits - 1;
                text.push_str(&format!("bit_{address:08x}_{i:03}_{k:02}\n"));
            }
        }
        out.write_all(text.as_bytes())?;
    }
    Ok(())
}

fn not_found(path: PathBuf) -> AssemblerError {
    // What `open()` reports: read the file to get the real error (e.g.
    // `IsADirectoryError` for a directory).
    let source = match std::fs::read(&path) {
        Err(e) => e,
        Ok(_) => io::Error::from_raw_os_error(2),
    };
    AssemblerError::Io { path, source }
}

fn run(
    db: &Database,
    assembler: &mut FasmAssembler<'_>,
    fasm: &Path,
    options: &Fasm2FramesOptions,
) -> Result<Frames, AssemblerError> {
    let info = db.part_info().ok_or_else(|| AssemblerError::Python {
        exception: "AttributeError",
        message: "the database was opened without a part".to_owned(),
    })?;
    let part_dir = db.root().join(&info.name);
    if info.package_pins.is_none() {
        return Err(not_found(part_dir.join("package_pins.csv")));
    }
    if info.iobanks.is_none() {
        let json = part_dir.join("part.json");
        if !json.is_file() {
            return Err(not_found(json));
        }
        return Err(AssemblerError::KeyError("iobanks".to_owned()));
    }

    let pudc = if options.emit_pudc_b_pullup {
        find_pudc_b(db)?
    } else {
        None
    };
    let pudc_in_use = Rc::new(Cell::new(false));
    if let Some((tile, site)) = &pudc {
        let (tile, site) = (tile.to_string(), site.clone());
        let in_use = Rc::clone(&pudc_in_use);
        assembler.set_feature_callback(Box::new(move |set_feature| {
            set_feature.feature.with_str(|feature| {
                let mut parts = feature.split('.');
                if parts.next() == Some(tile.as_str()) {
                    match parts.next() {
                        None => {
                            return Err(AssemblerError::Python {
                                exception: "IndexError",
                                message: "list index out of range".to_owned(),
                            })
                        }
                        Some(s) if s == site => in_use.set(true),
                        Some(_) => {}
                    }
                }
                Ok(())
            })
        }));
    }

    let mut extra_features: Vec<FasmLine> = Vec::new();
    if let Some(roi_path) = options.roi.as_deref().filter(|p| !p.as_os_str().is_empty()) {
        let design = read_roi_design(roi_path)?;
        assembler.mark_roi_frames(&design.roi);
        if let Some(text) = design.required_features {
            extra_features = fasm::parse_fasm_string(&text)?;
        }
    }
    let required = db.get_required_fasm_features(Some(&info.name)).join("\n");
    extra_features.extend(fasm::parse_fasm_string(&required)?);

    assembler.parse_fasm_filename(fasm, extra_features)?;

    if let Some((tile, site)) = pudc.as_ref().filter(|_| !pudc_in_use.get()) {
        let mut text = String::from("\n");
        for feature in PUDC_B_FEATURES {
            text.push_str(&format!("{tile}.{site}.{feature}\n"));
        }
        let mut missing = Vec::new();
        for line in fasm::parse_fasm_string(&text)? {
            assembler.add_fasm_line(line, &mut missing)?;
        }
        if !missing.is_empty() {
            return Err(AssemblerError::Lookup(missing));
        }
    }

    propagate_stepdown(db, assembler)?;
    assembler.get_frames(options.sparse)
}

/// `find_pudc_b(db)`: the tile and `IOB_Y<n>` site of the pin whose
/// function contains `PUDC_B`, if any.
///
/// # Errors
///
/// [`AssemblerError::Python`] `AssertionError` if there is more than one
/// such pin (the reference `assert`s), `ValueError`/`IndexError` if the
/// site name does not end with a digit.
pub fn find_pudc_b(db: &Database) -> Result<Option<(IdString, String)>, AssemblerError> {
    let Some(grid) = db.grid() else {
        return Ok(None);
    };
    let mut found: Option<(IdString, String)> = None;
    for tile in grid.tiles() {
        for &(site, function) in grid.pin_functions(tile) {
            if !function.with_str(|f| f.contains("PUDC_B")) {
                continue;
            }
            if let Some((t, s)) = &found {
                return Err(AssemblerError::Python {
                    exception: "AssertionError",
                    message: format!(
                        "(({}, {}), ({}, {}))",
                        py_repr(&t.to_string()),
                        py_repr(s),
                        py_repr(&tile.name.to_string()),
                        py_repr(&site.to_string())
                    ),
                });
            }
            let y = site.with_str(last_digit)?;
            found = Some((tile.name, format!("IOB_Y{}", y % 2)));
        }
    }
    Ok(found)
}

/// `int(site[-1])`.
fn last_digit(site: &str) -> Result<u32, AssemblerError> {
    let c = site.chars().last().ok_or_else(|| AssemblerError::Python {
        exception: "IndexError",
        message: "string index out of range".to_owned(),
    })?;
    c.to_digit(10).ok_or_else(|| AssemblerError::Python {
        exception: "ValueError",
        message: format!(
            "invalid literal for int() with base 10: {}",
            py_repr(&c.to_string())
        ),
    })
}

/// The STEPDOWN pass of `fasm2frames()`.
fn propagate_stepdown(
    db: &Database,
    assembler: &mut FasmAssembler<'_>,
) -> Result<(), AssemblerError> {
    let (Some(grid), Some(registry)) = (db.grid(), db.banks_tiles_registry()) else {
        return Ok(());
    };
    let mut used_iob_sites: HashSet<(String, String)> = HashSet::new();
    // (bank, tags), both in first seen order.
    let mut stepdown: Vec<(IdString, Vec<String>)> = Vec::new();
    for line in assembler.lines() {
        let Some(set_feature) = &line.set_feature else {
            continue;
        };
        if set_feature.value.is_zero() {
            continue;
        }
        set_feature.feature.with_str(|feature| {
            let mut parts = feature.splitn(3, '.');
            let (Some(tile), Some(site), Some(tag)) = (parts.next(), parts.next(), parts.next())
            else {
                return Ok::<(), AssemblerError>(());
            };
            if tile.contains("IOB33") {
                used_iob_sites.insert((tile.to_owned(), site.to_owned()));
            }
            if tag.contains("STEPDOWN") {
                let bank = IdString::lookup(tile)
                    .and_then(|t| registry.bank_of_tile(t))
                    .ok_or_else(|| AssemblerError::KeyError(tile.to_owned()))?;
                let index = match stepdown.iter().position(|(b, _)| *b == bank) {
                    Some(i) => i,
                    None => {
                        stepdown.push((bank, Vec::new()));
                        stepdown.len() - 1
                    }
                };
                let tags = &mut stepdown[index].1;
                if !tags.iter().any(|t| t == tag) {
                    tags.push(tag.to_owned());
                }
            }
            Ok(())
        })?;
    }

    let mut missing = Vec::new();
    for (bank, tags) in &stepdown {
        for &tile in registry.tiles_of_bank(*bank) {
            let name = tile.to_string();
            if name.contains("IOB33") {
                let grid_tile = grid
                    .tile(tile)
                    .ok_or_else(|| AssemblerError::KeyError(name.clone()))?;
                for &(site, _) in grid.sites(grid_tile) {
                    let site = format!("IOB_Y{}", site.with_str(last_digit)? % 2);
                    if used_iob_sites.contains(&(name.clone(), site.clone())) {
                        continue;
                    }
                    for tag in tags {
                        for line in fasm::parse_fasm_string(&format!("{name}.{site}.{tag}"))? {
                            assembler.add_fasm_line(line, &mut missing)?;
                        }
                    }
                }
            }
            if name.contains("HCLK_IOI3") {
                for line in fasm::parse_fasm_string(&format!("{name}.STEPDOWN"))? {
                    assembler.add_fasm_line(line, &mut missing)?;
                }
            }
        }
    }
    if missing.is_empty() {
        Ok(())
    } else {
        Err(AssemblerError::Lookup(missing))
    }
}

/// A ROI `design.json` (the output of prjxray's ROI harness): the grid
/// rectangle and the optional `required_features`.
#[derive(Clone, Debug, PartialEq)]
pub struct RoiDesign {
    /// `info.GRID_X_MIN` .. `info.GRID_Y_MAX`.
    pub roi: Roi,
    /// `'\n'.join(required_features)`, if the key exists.
    pub required_features: Option<String>,
}

/// The Python type name of a JSON value.
fn py_type(value: &Value) -> &'static str {
    match value {
        Value::Null => "NoneType",
        Value::Bool(_) => "bool",
        Value::Number(n) if n.is_f64() => "float",
        Value::Number(_) => "int",
        Value::String(_) => "str",
        Value::Array(_) => "list",
        Value::Object(_) => "dict",
    }
}

fn type_error(message: String) -> AssemblerError {
    AssemblerError::Python {
        exception: "TypeError",
        message,
    }
}

/// `value[key]` for a JSON object, with Python's errors.
fn subscript<'a>(value: &'a Value, key: &str) -> Result<&'a Value, AssemblerError> {
    match value {
        Value::Object(map) => map
            .get(key)
            .ok_or_else(|| AssemblerError::KeyError(key.to_owned())),
        Value::Array(_) => Err(type_error(
            "list indices must be integers or slices, not str".to_owned(),
        )),
        Value::String(_) => Err(type_error(
            "string indices must be integers, not 'str'".to_owned(),
        )),
        other => Err(type_error(format!(
            "'{}' object is not subscriptable",
            py_type(other)
        ))),
    }
}

/// Reads a ROI `design.json` like `fasm2frames()`: `info.GRID_X_MIN`,
/// `GRID_X_MAX`, `GRID_Y_MIN`, `GRID_Y_MAX` (numbers) and the optional
/// `required_features` (joined with `\n` like `'\n'.join(...)`: a list of
/// strings, or the characters of a string, or the keys of an object).
///
/// # Errors
///
/// [`AssemblerError::Io`], [`AssemblerError::Json`],
/// [`AssemblerError::KeyError`] for a missing key and
/// [`AssemblerError::Python`] `TypeError` for values of the wrong type.
pub fn read_roi_design(path: &Path) -> Result<RoiDesign, AssemblerError> {
    let data = std::fs::read(path).map_err(|source| AssemblerError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let json: Value = serde_json::from_slice(&data).map_err(|e| AssemblerError::Json {
        path: path.to_path_buf(),
        message: e.to_string(),
    })?;
    let info = subscript(&json, "info")?;
    let mut bounds = [0.0f64; 4];
    let values = [
        subscript(info, "GRID_X_MIN")?,
        subscript(info, "GRID_X_MAX")?,
        subscript(info, "GRID_Y_MIN")?,
        subscript(info, "GRID_Y_MAX")?,
    ];
    for (i, value) in values.iter().enumerate() {
        bounds[i] = match value {
            Value::Number(n) => n.as_f64().unwrap_or(f64::NAN),
            Value::Bool(b) => f64::from(u8::from(*b)),
            other => {
                // `x1 <= x`, `x <= x2`, `y1 <= y`, `y <= y2`.
                let (left, right) = if i % 2 == 0 {
                    (py_type(other), "int")
                } else {
                    ("int", py_type(other))
                };
                return Err(type_error(format!(
                    "'<=' not supported between instances of '{left}' and '{right}'"
                )));
            }
        };
    }
    let roi = Roi {
        x1: bounds[0],
        x2: bounds[1],
        y1: bounds[2],
        y2: bounds[3],
    };
    let required_features = match &json {
        Value::Object(map) => match map.get("required_features") {
            // `serde_json::Map` is sorted: take the keys of an object in
            // file order (a Python `dict` keeps insertion order).
            Some(Value::Object(_)) => Some(required_feature_keys(&data).join("\n")),
            Some(value) => Some(join_lines(value)?),
            None => None,
        },
        _ => None,
    };
    Ok(RoiDesign {
        roi,
        required_features,
    })
}

/// The keys of the ROI JSON's `required_features` object in file order,
/// like the Python `dict` `json.load` makes: a repeated key keeps its first
/// position, a repeated `required_features` the last value. Empty if it is
/// not an object (or the text is not valid JSON, which the caller has
/// already reported).
fn required_feature_keys(data: &[u8]) -> Vec<String> {
    use serde::de::{Deserializer, IgnoredAny, MapAccess, SeqAccess, Visitor};

    /// The keys of an object, in order, without duplicates (empty for any
    /// other value).
    struct Keys(Vec<String>);

    impl<'de> serde::Deserialize<'de> for Keys {
        fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
            struct V;
            impl<'de> Visitor<'de> for V {
                type Value = Keys;
                fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    f.write_str("any JSON value")
                }
                fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Keys, A::Error> {
                    let mut keys: Vec<String> = Vec::new();
                    while let Some((key, IgnoredAny)) = map.next_entry::<String, IgnoredAny>()? {
                        if !keys.contains(&key) {
                            keys.push(key);
                        }
                    }
                    Ok(Keys(keys))
                }
                fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Keys, A::Error> {
                    while seq.next_element::<IgnoredAny>()?.is_some() {}
                    Ok(Keys(Vec::new()))
                }
                fn visit_bool<E>(self, _: bool) -> Result<Keys, E> {
                    Ok(Keys(Vec::new()))
                }
                fn visit_i64<E>(self, _: i64) -> Result<Keys, E> {
                    Ok(Keys(Vec::new()))
                }
                fn visit_u64<E>(self, _: u64) -> Result<Keys, E> {
                    Ok(Keys(Vec::new()))
                }
                fn visit_f64<E>(self, _: f64) -> Result<Keys, E> {
                    Ok(Keys(Vec::new()))
                }
                fn visit_str<E>(self, _: &str) -> Result<Keys, E> {
                    Ok(Keys(Vec::new()))
                }
                fn visit_unit<E>(self) -> Result<Keys, E> {
                    Ok(Keys(Vec::new()))
                }
            }
            deserializer.deserialize_any(V)
        }
    }

    /// The top level object: the keys of its last `required_features`.
    struct Top(Vec<String>);

    impl<'de> serde::Deserialize<'de> for Top {
        fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
            struct V;
            impl<'de> Visitor<'de> for V {
                type Value = Top;
                fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    f.write_str("a JSON object")
                }
                fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Top, A::Error> {
                    let mut keys = Vec::new();
                    while let Some(key) = map.next_key::<String>()? {
                        if key == "required_features" {
                            keys = map.next_value::<Keys>()?.0;
                        } else {
                            map.next_value::<IgnoredAny>()?;
                        }
                    }
                    Ok(Top(keys))
                }
            }
            deserializer.deserialize_map(V)
        }
    }

    serde_json::from_slice::<Top>(data).map_or_else(|_| Vec::new(), |top| top.0)
}

/// `'\n'.join(value)`.
fn join_lines(value: &Value) -> Result<String, AssemblerError> {
    let items: Vec<String> = match value {
        Value::Array(items) => items
            .iter()
            .enumerate()
            .map(|(i, item)| match item {
                Value::String(s) => Ok(s.clone()),
                other => Err(type_error(format!(
                    "sequence item {i}: expected str instance, {} found",
                    py_type(other)
                ))),
            })
            .collect::<Result<_, _>>()?,
        Value::String(s) => s.chars().map(String::from).collect(),
        Value::Object(map) => map.keys().cloned().collect(),
        _ => return Err(type_error("can only join an iterable".to_owned())),
    };
    Ok(items.join("\n"))
}

/// `dump_frames_sparse(frames)` of `xc_fasm/fasm2frames.py` (the
/// `--debug` output): a blank line, `Frames: <n>`, then for every frame
/// with a non zero word `Frame @ 0x%08X` and `  % 3d: 0x%08X` per non
/// zero word.
///
/// # Errors
///
/// The errors of `out`.
pub fn dump_frames_sparse(frames: &Frames, out: &mut dyn Write) -> io::Result<()> {
    writeln!(out)?;
    writeln!(out, "Frames: {}", frames.len())?;
    for (address, words) in frames.iter() {
        if words.iter().all(|&w| w == 0) {
            continue;
        }
        writeln!(out, "Frame @ 0x{address:08X}")?;
        for (i, &word) in words.iter().enumerate() {
            if word != 0 {
                // `'% 3d' % i`: a space for the sign, width 3 in total.
                writeln!(out, "  {:>3}: 0x{word:08X}", format!(" {i}"))?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_dump_format() {
        let mut frames = Frames::zeroed(101, [0x10, 0x20]);
        frames.get_mut(0x20).unwrap()[5] = 0xAB;
        frames.get_mut(0x20).unwrap()[100] = 1;
        let mut out = Vec::new();
        dump_frames_sparse(&frames, &mut out).unwrap();
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "\nFrames: 2\nFrame @ 0x00000020\n    5: 0x000000AB\n   100: 0x00000001\n"
        );
    }

    #[test]
    fn prjuray_halfword_outputs() {
        let mut frames = Frames::zeroed(2, [0x100, 0x2_0000]);
        frames.get_mut(0x100).unwrap()[1] = 0x8001_0004;
        let mut frm = Vec::new();
        write_frm_halfwords(&frames, &mut frm).unwrap();
        assert_eq!(
            String::from_utf8(frm).unwrap(),
            "0x00000100 0x00000000,0x00000000,0x00000004,0x00008001\n\
             0x00020000 0x00000000,0x00000000,0x00000000,0x00000000\n"
        );
        let mut dump = Vec::new();
        dump_frames_sparse_halfwords(&frames, &mut dump).unwrap();
        assert_eq!(
            String::from_utf8(dump).unwrap(),
            "\nFrames: 2\nFrame @ 0x00000100\n    2: 0x00000004\n    3: 0x00008001\n"
        );
        let mut bits = Vec::new();
        write_bits(&frames, &mut bits).unwrap();
        assert_eq!(
            String::from_utf8(bits).unwrap(),
            "bit_00000100_001_02\nbit_00000100_001_16\nbit_00000100_001_31\n"
        );
    }

    #[test]
    fn join_like_python() {
        let v: Value = serde_json::from_str(r#"["a", "b"]"#).unwrap();
        assert_eq!(join_lines(&v).unwrap(), "a\nb");
        let v: Value = serde_json::from_str(r#""ab""#).unwrap();
        assert_eq!(join_lines(&v).unwrap(), "a\nb");
        let v: Value = serde_json::from_str(r#"["a", 1]"#).unwrap();
        assert_eq!(
            join_lines(&v).unwrap_err().to_string(),
            "sequence item 1: expected str instance, int found"
        );
        let v: Value = serde_json::from_str("3").unwrap();
        assert!(join_lines(&v).is_err());
    }

    #[test]
    fn required_features_object_keeps_file_order() {
        let keys = |text: &str| required_feature_keys(text.as_bytes());
        assert_eq!(
            keys(r#"{"info": {}, "required_features": {"B": 1, "A": 2, "B": 3, "C": null}}"#),
            ["B", "A", "C"]
        );
        // The last `required_features` wins.
        assert_eq!(
            keys(r#"{"required_features": {"X": 1}, "required_features": {"Z": 1, "Y": 2}}"#),
            ["Z", "Y"]
        );
        assert!(keys(r#"{"required_features": ["A"]}"#).is_empty());
        assert!(keys("[1]").is_empty());
    }

    #[test]
    fn site_digits() {
        assert_eq!(last_digit("IOB_X0Y13").unwrap(), 3);
        assert_eq!(
            last_digit("IOB_X").unwrap_err().traceback_line(),
            "ValueError: invalid literal for int() with base 10: 'X'"
        );
        assert_eq!(
            last_digit("").unwrap_err().traceback_line(),
            "IndexError: string index out of range"
        );
    }
}
