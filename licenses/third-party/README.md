# Third-party license record

The Rust application uses Slint under GPL-3.0-only. The upstream reference
project keeps its Apache-2.0 license. See the root LICENSE and NOTICE.

inventory.json and inventory.csv identify the locked Windows x64 source and
build dependencies. They do not assert that all listed packages are linked into
the executable. THIRD-PARTY-NOTICES.txt combines the collected texts; texts/
preserves their original bytes under SHA256 filenames.

For packages without license files in the Cargo source archive,
upstream-sources.json preserves the exact upstream root license/author files
at the crate's recorded VCS revision. The base64 payloads preserve bytes, and
the collector verifies each Git blob SHA1 before including it. gl-rs moved
from brendanzab/gl-rs to rust-windowing/gl-rs; the recorded commits are unchanged.

Regenerate from the repository with the bundled Python or another Python 3:

    python tools/collect-licenses.py evidence/functional/license-inventory/windows-x64-metadata.json licenses/third-party

The metadata was obtained using cargo metadata --format-version 1 --locked
--offline --filter-platform x86_64-pc-windows-msvc. Its digest is recorded in
inventory.json. The source directories referenced by that metadata must exist
to regenerate the record.

All 369 inventoried packages now have collected texts (246 distinct byte
sequences). This is a collection checkpoint. Final release notices, corresponding
source packaging, and a clean-directory rebuild remain release acceptance work.
No public upload or release is authorized by this record.
