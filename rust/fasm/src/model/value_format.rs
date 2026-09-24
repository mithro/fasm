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

//! [`ValueFormat`]: the number format used to print a FASM value.

use super::error::ModelError;

/// Number format used for a FASM value, mirroring Python's
/// `fasm.model.ValueFormat` enum (same discriminant values, so the two
/// convert with a plain cast).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[repr(u8)]
pub enum ValueFormat {
    /// A decimal number without size or radix, e.g. `42`.
    Plain = 0,
    /// A decimal number with optional size, e.g. `8'd42`.
    VerilogDecimal = 1,
    /// A hexadecimal number with optional size, e.g. `8'h2a`.
    VerilogHex = 2,
    /// A binary number with optional size, e.g. `8'b00101010`.
    VerilogBinary = 3,
    /// An octal number with optional size, e.g. `8'o52`.
    VerilogOctal = 4,
}

impl ValueFormat {
    /// The name of the corresponding Python `ValueFormat` enum member, e.g.
    /// `"VERILOG_HEX"`.
    #[must_use]
    pub fn python_name(self) -> &'static str {
        match self {
            ValueFormat::Plain => "PLAIN",
            ValueFormat::VerilogDecimal => "VERILOG_DECIMAL",
            ValueFormat::VerilogHex => "VERILOG_HEX",
            ValueFormat::VerilogBinary => "VERILOG_BINARY",
            ValueFormat::VerilogOctal => "VERILOG_OCTAL",
        }
    }

    /// The numeric base this format prints its digits in.
    #[must_use]
    pub fn radix(self) -> u32 {
        match self {
            ValueFormat::Plain | ValueFormat::VerilogDecimal => 10,
            ValueFormat::VerilogHex => 16,
            ValueFormat::VerilogBinary => 2,
            ValueFormat::VerilogOctal => 8,
        }
    }
}

impl TryFrom<u8> for ValueFormat {
    type Error = ModelError;

    /// Converts a Python `ValueFormat.value` discriminant back into a
    /// `ValueFormat`.
    ///
    /// # Errors
    ///
    /// Returns [`ModelError::InvalidValueFormat`] if `v` is not `0..=4`.
    fn try_from(v: u8) -> Result<Self, Self::Error> {
        match v {
            0 => Ok(ValueFormat::Plain),
            1 => Ok(ValueFormat::VerilogDecimal),
            2 => Ok(ValueFormat::VerilogHex),
            3 => Ok(ValueFormat::VerilogBinary),
            4 => Ok(ValueFormat::VerilogOctal),
            _ => Err(ModelError::InvalidValueFormat(v)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discriminants_match_python() {
        assert_eq!(ValueFormat::Plain as u8, 0);
        assert_eq!(ValueFormat::VerilogDecimal as u8, 1);
        assert_eq!(ValueFormat::VerilogHex as u8, 2);
        assert_eq!(ValueFormat::VerilogBinary as u8, 3);
        assert_eq!(ValueFormat::VerilogOctal as u8, 4);
    }

    #[test]
    fn python_names() {
        assert_eq!(ValueFormat::Plain.python_name(), "PLAIN");
        assert_eq!(ValueFormat::VerilogDecimal.python_name(), "VERILOG_DECIMAL");
        assert_eq!(ValueFormat::VerilogHex.python_name(), "VERILOG_HEX");
        assert_eq!(ValueFormat::VerilogBinary.python_name(), "VERILOG_BINARY");
        assert_eq!(ValueFormat::VerilogOctal.python_name(), "VERILOG_OCTAL");
    }

    #[test]
    fn radixes() {
        assert_eq!(ValueFormat::Plain.radix(), 10);
        assert_eq!(ValueFormat::VerilogDecimal.radix(), 10);
        assert_eq!(ValueFormat::VerilogHex.radix(), 16);
        assert_eq!(ValueFormat::VerilogBinary.radix(), 2);
        assert_eq!(ValueFormat::VerilogOctal.radix(), 8);
    }

    #[test]
    fn try_from_round_trip() {
        for v in 0u8..=4 {
            let format = ValueFormat::try_from(v).unwrap();
            assert_eq!(format as u8, v);
        }
    }

    #[test]
    fn try_from_rejects_out_of_range() {
        assert_eq!(
            ValueFormat::try_from(5),
            Err(ModelError::InvalidValueFormat(5))
        );
        assert_eq!(
            ValueFormat::try_from(255),
            Err(ModelError::InvalidValueFormat(255))
        );
    }
}
