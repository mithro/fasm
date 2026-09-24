# vtr corpus

VTR (verilog-to-routing) keeps **no committed `.fasm` files**: its
`genfasm`/`FasmWriterVisitor` writes FASM lines at runtime from
architecture `fasm_*` metadata plus the routed/packed netlist. What it does
commit is a unit test of that writer (`utils/fasm/test/test_fasm.cpp`,
Catch2) and the tiny synthetic architecture it runs against
(`utils/fasm/test/test_fasm_arch.xml`). `vtr_test_fasm_literals.fasm`
extracts every literal FASM feature string (and, where the metadata itself
uses `{tag}` placeholders that VTR fills in per tile instance, the
substituted forms for two representative tile instances) findable in those
two files into one committed FASM fixture; see the comments inside that
file for exactly where each line came from and how any placeholder was
substituted.

## Origin

* Repository: <https://github.com/verilog-to-routing/vtr-verilog-to-routing>
* Commit: `34f65cb7c14dd4a59d5b95a5d65920dcc0d9a89b` (from a local read-only,
  blob-less checkout at session time; `git -C <checkout> rev-parse HEAD`).
* Licence: MIT (see `LICENSE.md` in that repository; VTR itself notes ABC,
  benchmark circuits and some libraries are under other licences, none of
  which are involved here -- `utils/fasm/` is VTR's own MIT licensed code).
* Paths: `utils/fasm/test/test_fasm.cpp`, `utils/fasm/test/test_fasm_arch.xml`.

No files from that checkout are copied verbatim; `vtr_test_fasm_literals.fasm`
is a hand written extraction (see its header comment) of the literal and
placeholder-substituted FASM strings found in those two files, done by
reading them at the commit above.

## Files

* `vtr_test_fasm_literals.fasm`: the extracted/substituted FASM feature
  lines described above, one feature per line, each preceded by a comment
  giving its exact source (test case name / XML element / metadata
  attribute) and, where relevant, the placeholder substitution used.
