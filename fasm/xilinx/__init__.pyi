# Copyright 2017-2022 F4PGA Authors
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# You may obtain a copy of the License at
#
#     http://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.
#
# SPDX-License-Identifier: Apache-2.0
"""Type stubs of fasm.xilinx (see fasm/xilinx/__init__.py and the
docstrings of the extension's classes for the documentation)."""

import os
from typing import (
    IO, Callable, Dict, Iterable, Iterator, List, Mapping, NamedTuple,
    Optional, Sequence, Tuple, Union)

from fasm.model import FasmLine, SetFasmFeature
from fasm.xilinx._types import (
    BitstreamError as BitstreamError,
    DbError as DbError,
    Error as Error,
    FasmInconsistentBits as FasmInconsistentBits,
    FasmKeyError as FasmKeyError,
    FasmLookupError as FasmLookupError,
    FasmParseError as FasmParseError,
    FrmError as FrmError,
)

_Path = Union[str, bytes, 'os.PathLike[str]', 'os.PathLike[bytes]']
_Lines = Union[str, bytes, Iterable[Union[FasmLine, str]]]

ARCHITECTURES: Tuple[str, str, str]

__all__ = [
    'ARCHITECTURES', 'BitstreamError', 'Database', 'DbError', 'Error',
    'FasmAssembler', 'FasmInconsistentBits', 'FasmKeyError',
    'FasmLookupError', 'FasmParseError', 'FeatureBits', 'Frames', 'FrmError',
    'Roi', 'RoiDesign', 'Tile', 'dump_frames_sparse', 'fasm2bit',
    'fasm2frames', 'read_bitstream', 'read_roi_design', 'write_bitstream'
]


class Tile(NamedTuple):
    name: str
    tile_type: str
    grid_x: int
    grid_y: int


class Roi(NamedTuple):
    x1: float
    x2: float
    y1: float
    y2: float


class RoiDesign(NamedTuple):
    roi: Roi
    required_features: Optional[str]


class FeatureBits(NamedTuple):
    tile: str
    tile_type: Optional[str]
    segbits_tile_type: Optional[str]
    pseudo_pip: Optional[str]
    block_type: Optional[str]
    base_address: Optional[int]
    frame_count: Optional[int]
    offset: Optional[int]
    # (frame_address, word, bit, value)
    bits: Tuple[Tuple[int, int, int, bool], ...]


class Database:
    def __init__(
            self,
            db_root: _Path,
            part: Optional[str] = ...,
            cache: Union[bool, None, _Path] = ...) -> None:
        ...

    @staticmethod
    def open(
            db_root: _Path,
            part: Optional[str] = ...,
            cache: Union[bool, None, _Path] = ...) -> 'Database':
        ...

    @property
    def root(self) -> str:
        ...

    @property
    def part(self) -> Optional[str]:
        ...

    @property
    def layout(self) -> str:
        ...

    @property
    def architecture(self) -> str:
        ...

    @property
    def words_per_frame(self) -> int:
        ...

    @property
    def idcode(self) -> Optional[int]:
        ...

    def tile_types(self) -> List[str]:
        ...

    def tile_type_features(self, tile_type: str) -> List[str]:
        ...

    def pseudo_pips(self, tile_type: str) -> List[Tuple[str, str]]:
        ...

    def tiles(self) -> List[Tile]:
        ...

    def required_features(self) -> List[str]:
        ...

    def frame_addresses(self) -> Optional[List[int]]:
        ...

    def lookup_feature(self, feature: str, address: int = ...) -> FeatureBits:
        ...


class Frames(Mapping[int, List[int]]):
    def __init__(
            self,
            frames: Optional[Mapping[int, Union[Sequence[int], bytes]]] = ...,
            words_per_frame: Optional[int] = ...) -> None:
        ...

    @property
    def words_per_frame(self) -> int:
        ...

    def __getitem__(self, address: int) -> List[int]:
        ...

    def __iter__(self) -> Iterator[int]:
        ...

    def __len__(self) -> int:
        ...

    def keys(self) -> List[int]:  # type: ignore[override]
        ...

    def values(self) -> List[List[int]]:  # type: ignore[override]
        ...

    def items(  # type: ignore[override]
            self) -> List[Tuple[int, List[int]]]:
        ...

    def to_dict(self) -> Dict[int, List[int]]:
        ...

    def frame_bytes(self, address: int) -> bytes:
        ...

    def to_bytes(self) -> bytes:
        ...

    def set_bits(self) -> List[Tuple[int, int, int]]:
        ...

    def to_frm(self) -> str:
        ...

    def write_frm(self, target: Union[_Path, IO[str], IO[bytes]]) -> None:
        ...

    @staticmethod
    def from_frm(
            data: Union[str, bytes], words_per_frame: int = ...) -> 'Frames':
        ...

    @staticmethod
    def read_frm(
            source: Union[_Path, IO[str], IO[bytes]],
            words_per_frame: int = ...) -> 'Frames':
        ...


class FasmAssembler:
    def __init__(self, db: Database, prjuray: Optional[bool] = ...) -> None:
        ...

    @property
    def database(self) -> Database:
        ...

    def parse_fasm_filename(
            self, filename: _Path,
            extra_features: Optional[_Lines] = ...) -> None:
        ...

    def parse_fasm_string(
            self, text: str, extra_features: Optional[_Lines] = ...) -> None:
        ...

    def parse_fasm_bytes(
            self, data: bytes,
            extra_features: Optional[_Lines] = ...) -> None:
        ...

    def add_fasm_line(
            self,
            line: Union[FasmLine, str],
            missing_features: Optional[List[str]] = ...) -> None:
        ...

    def add_fasm_lines(
            self, lines: _Lines,
            missing_features: Optional[List[str]] = ...) -> None:
        ...

    def add_required_features(self) -> None:
        ...

    def mark_roi_frames(
            self, roi: Union[Roi, Sequence[float]]) -> None:
        ...

    def propagate_stepdown(self) -> None:
        ...

    def set_feature_callback(
            self, callback: Optional[Callable[[SetFasmFeature], object]]
    ) -> None:
        ...

    def get_frames(self, sparse: bool = ...) -> Frames:
        ...

    @property
    def warnings(self) -> List[str]:
        ...

    def take_warnings(self) -> List[str]:
        ...

    def __len__(self) -> int:
        ...


_Frames = Union[Frames, Mapping[int, Union[Sequence[int], bytes]]]
_Part = Union[Database, _Path]


def write_bitstream(
        frames: _Frames,
        part: _Part,
        output: Union[None, _Path, IO[bytes]] = ...,
        *,
        format: Optional[str] = ...,
        part_name: Optional[str] = ...,
        design_name: Optional[_Path] = ...,
        generator: Optional[str] = ...,
        source_date_epoch: Optional[int] = ...) -> Optional[bytes]:
    ...


def read_bitstream(
        source: Union[bytes, bytearray, _Path, IO[bytes]],
        part: _Part,
        *,
        format: Optional[str] = ...,
        clear_ecc: bool = ...,
        skip_zero: bool = ...) -> Frames:
    ...


def read_roi_design(path: _Path) -> RoiDesign:
    ...


def dump_frames_sparse(frames: _Frames) -> str:
    ...


def fasm2frames(
        db_root: Union[_Path, Database],
        part: Optional[str] = ...,
        filename_in: Optional[_Path] = ...,
        f_out: Union[None, _Path, IO[str], IO[bytes]] = ...,
        sparse: bool = ...,
        roi: Optional[_Path] = ...,
        debug: bool = ...,
        emit_pudc_b_pullup: bool = ...,
        fasm_text: Union[None, str, bytes] = ...,
        cache: Union[bool, None, _Path] = ...) -> Frames:
    ...


def fasm2bit(
        db_root: Union[_Path, Database],
        part: Optional[str],
        fn_in: Optional[_Path],
        bit_out: Union[None, _Path, IO[bytes]],
        part_file: Optional[_Path] = ...,
        frm_out: Union[None, _Path, IO[str], IO[bytes]] = ...,
        sparse: bool = ...,
        roi: Optional[_Path] = ...,
        debug: bool = ...,
        emit_pudc_b_pullup: bool = ...,
        fasm_text: Union[None, str, bytes] = ...,
        cache: Union[bool, None, _Path] = ...,
        format: Optional[str] = ...,
        source_date_epoch: Optional[int] = ...
) -> Union[Frames, Tuple[Frames, bytes]]:
    ...
