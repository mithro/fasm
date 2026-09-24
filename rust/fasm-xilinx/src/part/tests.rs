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

use super::*;
use crate::arch::FrameAddressFields;

/// A Series7 part: top rows 0 and 1, bottom row 0; CLB_IO_CLK columns
/// 0..=2 (row 1 lacks BLOCK_RAM), BLOCK_RAM columns 0..=1.
const SERIES7_YAML: &str = "\
!<xilinx/xc7series/part>
idcode: 0x362d093
global_clock_regions:
  top: !<xilinx/xc7series/global_clock_region>
    rows:
      0: !<xilinx/xc7series/row>
        configuration_buses:
          CLB_IO_CLK: !<xilinx/xc7series/configuration_bus>
            configuration_columns:
              0: !<xilinx/xc7series/configuration_column>
                frame_count: 3
              1: !<xilinx/xc7series/configuration_column>
                frame_count: 2
              2: {frame_count: 1}
          BLOCK_RAM: !<xilinx/xc7series/configuration_bus>
            configuration_columns:
              0: {frame_count: 2}
              1: {frame_count: 1}
      1: !<xilinx/xc7series/row>
        configuration_buses:
          CLB_IO_CLK: !<xilinx/xc7series/configuration_bus>
            configuration_columns:
              0: {frame_count: 2}
  bottom: !<xilinx/xc7series/global_clock_region>
    rows:
      0: !<xilinx/xc7series/row>
        configuration_buses:
          CLB_IO_CLK: !<xilinx/xc7series/configuration_bus>
            configuration_columns:
              0: {frame_count: 1}
          BLOCK_RAM: !<xilinx/xc7series/configuration_bus>
            configuration_columns:
              0: {frame_count: 1}
";

const SERIES7_JSON: &str = r#"{
  "global_clock_regions": {
    "top": {"rows": {
      "0": {"configuration_buses": {
        "CLB_IO_CLK": {"configuration_columns": {"0": {"frame_count": 3}, "1": {"frame_count": 2}, "2": {"frame_count": 1}}},
        "BLOCK_RAM": {"configuration_columns": {"0": {"frame_count": 2}, "1": {"frame_count": 1}}}}},
      "1": {"configuration_buses": {
        "CLB_IO_CLK": {"configuration_columns": {"0": {"frame_count": 2}}}}}}},
    "bottom": {"rows": {
      "0": {"configuration_buses": {
        "CLB_IO_CLK": {"configuration_columns": {"0": {"frame_count": 1}}},
        "BLOCK_RAM": {"configuration_columns": {"0": {"frame_count": 1}}}}}}}},
  "idcode": 56807571,
  "iobanks": {"0": "X1Y78", "14": "X1Y26"}
}"#;

fn addr(block_type: u8, bottom: bool, row: u8, column: u16, minor: u16) -> FrameAddress {
    FrameAddress::compose(
        Architecture::Series7,
        FrameAddressFields {
            block_type,
            bottom,
            row,
            column,
            minor,
        },
    )
    .unwrap()
}

#[test]
fn series7_yaml_and_json_agree() {
    let yaml = Part::from_yaml_str(SERIES7_YAML, Architecture::UltraScalePlus).unwrap();
    assert_eq!(yaml.architecture, Architecture::Series7);
    assert_eq!(yaml.idcode, 0x362d093);
    let json: Json = serde_json::from_str(SERIES7_JSON).unwrap();
    let from_json = Part::from_json(&json, Architecture::UltraScalePlus)
        .unwrap()
        .unwrap();
    assert_eq!(yaml, from_json);
    assert_eq!(yaml.frame_count(), 13);
    assert_eq!(yaml.rows().len(), 3);
    assert_eq!(yaml.rows()[2].bus(1).unwrap().columns(), &[(0, 1)]);
}

#[test]
fn series7_frame_enumeration() {
    let part = Part::from_yaml_str(SERIES7_YAML, Architecture::Series7).unwrap();
    let frames: Vec<FrameAddress> = part.iter_frame_addresses().collect();
    let expected = vec![
        // CLB_IO_CLK top row 0.
        addr(0, false, 0, 0, 0),
        addr(0, false, 0, 0, 1),
        addr(0, false, 0, 0, 2),
        addr(0, false, 0, 1, 0),
        addr(0, false, 0, 1, 1),
        addr(0, false, 0, 2, 0),
        // Top row 1.
        addr(0, false, 1, 0, 0),
        addr(0, false, 1, 0, 1),
        // Bottom row 0.
        addr(0, true, 0, 0, 0),
        // BLOCK_RAM top row 0 (row 1 has no BLOCK_RAM bus: the region
        // ends there, prjxray goes to the bottom region).
        addr(1, false, 0, 0, 0),
        addr(1, false, 0, 0, 1),
        addr(1, false, 0, 1, 0),
        addr(1, true, 0, 0, 0),
    ];
    assert_eq!(frames, expected);
    assert!(frames.windows(2).all(|w| w[0] < w[1]));
    assert!(frames.iter().all(|&f| part.is_valid_frame_address(f)));
    assert!(!part.is_valid_frame_address(addr(0, false, 0, 0, 3)));
    assert!(!part.is_valid_frame_address(addr(0, false, 2, 0, 0)));
    assert!(!part.is_valid_frame_address(addr(2, false, 0, 0, 0)));
    // An address in an unknown top row continues with the bottom region
    // (Part::GetNextFrameAddress asks the region, then tries bottom row 0).
    assert_eq!(
        part.next_frame_address(addr(0, false, 5, 0, 0)),
        Some(addr(0, true, 0, 0, 0))
    );
    assert_eq!(part.next_frame_address(addr(1, true, 0, 0, 0)), None);
    // A minor beyond its column: ConfigurationColumn gives nothing and the
    // bus continues with the next column (then the next row, ...).
    assert_eq!(
        part.next_frame_address(addr(0, false, 0, 0, 9)),
        Some(addr(0, false, 0, 1, 0))
    );
    assert_eq!(
        part.next_frame_address(addr(0, false, 0, 2, 9)),
        Some(addr(0, false, 1, 0, 0))
    );
}

#[test]
fn from_frame_addresses() {
    let yaml = Part::from_yaml_str(SERIES7_YAML, Architecture::Series7).unwrap();
    let addresses: Vec<FrameAddress> = yaml.iter_frame_addresses().collect();
    let part = Part::from_frame_addresses(Architecture::Series7, yaml.idcode, addresses).unwrap();
    assert_eq!(part, yaml);
    assert!(Part::from_frame_addresses(Architecture::Series7, 1, [FrameAddress(7 << 23)]).is_err());
}

#[test]
fn row_gap_stops_the_region_like_prjxray() {
    // Rows 0 and 2 (no row 1): the next row of the map is row 2, which is
    // used because GetNextFrameAddress walks the map, not row + 1.
    let text = "\
!<xilinx/xc7series/part>
idcode: 1
global_clock_regions:
  top:
    rows:
      0: {configuration_buses: {CLB_IO_CLK: {configuration_columns: {0: {frame_count: 1}}}}}
      2: {configuration_buses: {CLB_IO_CLK: {configuration_columns: {3: {frame_count: 1}, 5: {frame_count: 1}}}}}
  bottom:
    rows: {}
";
    let part = Part::from_yaml_str(text, Architecture::Series7).unwrap();
    let frames: Vec<_> = part.iter_frame_addresses().collect();
    // Row 2 starts at column 3, so "column 0 of the next row" is not
    // valid and the walk stops after row 0 (a prjxray quirk kept as is).
    assert_eq!(frames, vec![addr(0, false, 0, 0, 0)]);
    assert_eq!(part.frame_count(), 3);
}

#[test]
fn address_zero_is_always_first() {
    // A part without frame 0 still yields 0 first (addMissingFrames),
    // then continues from it.
    let text = "idcode: 1\nglobal_clock_regions:\n  top:\n    rows: {}\n  bottom:\n    rows:\n      0: {configuration_buses: {CLB_IO_CLK: {configuration_columns: {0: {frame_count: 2}}}}}\n";
    let part = Part::from_yaml_str(text, Architecture::Series7).unwrap();
    assert!(!part.is_valid_frame_address(FrameAddress(0)));
    let frames: Vec<_> = part.iter_frame_addresses().collect();
    assert_eq!(
        frames,
        vec![
            FrameAddress(0),
            addr(0, true, 0, 0, 0),
            addr(0, true, 0, 0, 1)
        ]
    );
}

#[test]
fn ultrascale_plus_rows() {
    let text = "\
!<xilinx/xcupseries/part>
idcode: 0x4a42093
rows:
  0: !<xilinx/xcupseries/row>
    configuration_buses:
      CLB_IO_CLK: !<xilinx/xcupseries/configuration_bus>
        configuration_columns:
          0: !<xilinx/xcupseries/configuration_column>
            frame_count: 2
          1: {frame_count: 256}
      BLOCK_RAM: {configuration_columns: {0: {frame_count: 1}}}
  33:
    configuration_buses:
      CLB_IO_CLK: {configuration_columns: {0: {frame_count: 1}}}
";
    let part = Part::from_yaml_str(text, Architecture::Series7).unwrap();
    assert_eq!(part.architecture, Architecture::UltraScalePlus);
    let frames: Vec<_> = part.iter_frame_addresses().collect();
    assert_eq!(frames.len(), 2 + 256 + 1 + 1);
    let arch = Architecture::UltraScalePlus;
    // Row 33 is row 1 of the bottom half.
    let last_clb = frames[2 + 256];
    assert!(last_clb.is_bottom_half(arch));
    assert_eq!(last_clb.row(arch), 1);
    assert_eq!(last_clb.row_index(arch), 33);
    assert_eq!(frames[2 + 255].minor(arch), 255);
    assert_eq!(
        frames.last().unwrap().block_type(arch),
        Some(BlockType::BlockRam)
    );
    assert!(frames.windows(2).all(|w| w[0] < w[1]));

    let json: Json = serde_json::from_str(
        r#"{"idcode": 77865107, "rows": {"0": {"configuration_buses": {"CLB_IO_CLK": {"configuration_columns": {"0": {"frame_count": 2}, "1": {"frame_count": 256}}}, "BLOCK_RAM": {"configuration_columns": {"0": {"frame_count": 1}}}}}, "33": {"configuration_buses": {"CLB_IO_CLK": {"configuration_columns": {"0": {"frame_count": 1}}}}}}}"#,
    )
    .unwrap();
    let from_json = Part::from_json(&json, arch).unwrap().unwrap();
    assert_eq!(from_json.rows(), part.rows());
}

#[test]
fn invalid_parts() {
    let cases = [
        ("idcode: 1\nconfiguration_ranges: {}\n", "configuration_ranges"),
        ("idcode: 1\n", "no global_clock_regions"),
        ("!<xilinx/other/part>\nidcode: 1\n", "unknown part tag"),
        ("global_clock_regions: {top: {}, bottom: {}}\n", "idcode"),
        ("idcode: x\nglobal_clock_regions: {top: {}, bottom: {}}\n", "not an unsigned"),
        ("idcode: 1\nglobal_clock_regions: {top: {}}\n", "bottom"),
        (
            "idcode: 1\nglobal_clock_regions: {bottom: {}, top: {rows: {32: {}}}}\n",
            "row 32",
        ),
        (
            "idcode: 1\nglobal_clock_regions: {bottom: {}, top: {rows: {0: {configuration_buses: {FOO: {}}}}}}\n",
            "unknown block type",
        ),
        (
            "idcode: 1\nglobal_clock_regions: {bottom: {}, top: {rows: {0: {configuration_buses: {CLB_IO_CLK: {configuration_columns: {0: {frame_count: 129}}}}}}}}\n",
            "frame_count 129",
        ),
        (
            "idcode: 1\nglobal_clock_regions: {bottom: {}, top: {rows: {0: {configuration_buses: {CLB_IO_CLK: {configuration_columns: {1024: {frame_count: 1}}}}}}}}\n",
            "column 1024",
        ),
        (
            "idcode: 1\nglobal_clock_regions: {bottom: {}, top: {rows: {0: {configuration_buses: {CLB_IO_CLK: {configuration_columns: {00: {frame_count: 1}, 0: {frame_count: 1}}}}}}}}\n",
            "listed twice",
        ),
        (
            "idcode: 1\nglobal_clock_regions: {bottom: {}, top: {rows: {0: {configuration_buses: {CLB_IO_CLK: {configuration_columns: {a: {frame_count: 1}}}}}}}}\n",
            "not an unsigned integer",
        ),
    ];
    for (text, message) in cases {
        let err = Part::from_yaml_str(text, Architecture::Series7).unwrap_err();
        assert!(err.message.contains(message), "{text:?}: {err}");
    }
    let json = |s: &str| -> Json { serde_json::from_str(s).unwrap() };
    assert_eq!(
        Part::from_json(&json(r#"{"iobanks": {}}"#), Architecture::Series7),
        Ok(None)
    );
    for (text, message) in [
        (r#"{"rows": {}}"#, "no idcode"),
        (
            r#"{"rows": {"0": {"configuration_buses": {"CLB_IO_CLK": {"configuration_columns": {"0": {}}}}}}, "idcode": 1}"#,
            "frame_count",
        ),
        (r#"{"rows": [], "idcode": 1}"#, "not an object"),
        (r#"{"rows": {}, "idcode": -1}"#, "idcode"),
        (
            r#"{"global_clock_regions": {"top": {}}, "idcode": 1}"#,
            "bottom",
        ),
    ] {
        let err = Part::from_json(&json(text), Architecture::UltraScalePlus).unwrap_err();
        assert!(err.contains(message), "{text}: {err}");
    }
}

#[test]
fn package_pins_csv() {
    let text = "pin,bank,site,tile,pin_function\nA1,35,IOB_X1Y81,RIOB33_X43Y81,IO_L9N_T1_DQS_AD7N_35\n\n\"B,2\",14,IOB_X0Y1,\"LIOB33_X0Y1\",\"a\"\"b\"\r\n";
    let pins = parse_package_pins(text).unwrap();
    assert_eq!(pins.len(), 2);
    assert_eq!(pins[0].tile, "RIOB33_X43Y81");
    assert_eq!(pins[0].bank, "35");
    assert_eq!(pins[0].pin_function, "IO_L9N_T1_DQS_AD7N_35");
    assert_eq!(pins[1].pin, "B,2");
    assert_eq!(pins[1].pin_function, "a\"b");
    // Other column orders and missing optional columns.
    let pins = parse_package_pins("tile,bank\nT_X0Y0,7\n").unwrap();
    assert_eq!(
        (pins[0].tile, pins[0].bank, pins[0].pin),
        (
            IdString::new("T_X0Y0"),
            IdString::new("7"),
            IdString::new("")
        )
    );
    assert_eq!(parse_package_pins("").unwrap(), vec![]);
    for (text, line) in [
        ("pin,bank\nA1,1\n", 1),
        ("pin,bank,tile\nA1,1\n", 2),
        ("pin,bank,tile\nA1,1,\"x\n", 2),
    ] {
        assert_eq!(parse_package_pins(text).unwrap_err().0, line, "{text:?}");
    }
}

#[test]
fn banks_registry() {
    let id = IdString::new;
    let pin = |bank: &str, tile: &str| PackagePin {
        pin: id(""),
        bank: id(bank),
        site: id(""),
        tile: id(tile),
        pin_function: id(""),
    };
    let registry = BanksTilesRegistry::new(
        &[(id("99"), id("X1Y26")), (id("66"), id("X113Y26"))],
        &[
            pin("99", "LIOB33_X0Y1"),
            pin("99", "LIOB33_X0Y1"),
            pin("99", "LIOB33_SING_X0Y0"),
            pin("66", "RIOB33_X43Y1"),
        ],
    );
    assert_eq!(
        registry.tiles_of_bank(id("99")),
        &[
            id("HCLK_IOI3_X1Y26"),
            id("LIOB33_X0Y1"),
            id("LIOB33_SING_X0Y0")
        ]
    );
    assert_eq!(registry.bank_of_tile(id("RIOB33_X43Y1")), Some(id("66")));
    assert_eq!(
        registry.bank_of_tile(id("HCLK_IOI3_X113Y26")),
        Some(id("66"))
    );
    assert_eq!(registry.bank_of_tile(id("NOPE")), None);
    assert!(registry.tiles_of_bank(id("1")).is_empty());
    assert_eq!(registry.banks().collect::<Vec<_>>(), [id("99"), id("66")]);
}
