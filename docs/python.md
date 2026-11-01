# Python bindings (draft)

The optional `python` Cargo feature exposes `convert` and `book_info` through
PyO3 0.30. The maturin build disables default features, retaining `parallel`
without the CLI or image optimization dependencies. Ordinary Rust and WASM
builds do not need Python.

## Local development

Requires CPython 3.11+ and Rust 1.91+.

```sh
uv venv --python 3.11
source .venv/bin/activate
uv pip install 'maturin>=1.16,<2'
maturin develop --uv --release
python -m unittest discover -s tests/python -v
PYO3_PYTHON="$VIRTUAL_ENV/bin/python" PYTHONHOME="$(python -c 'import sys; print(sys.base_prefix)')" \
  cargo test --no-default-features --features python,parallel --lib python::tests
```

On Windows, activate with `.venv\Scripts\Activate.ps1` and use its Python path
for `PYO3_PYTHON` instead. `PYTHONHOME` supplies the standard library location for
Rust's embedded-interpreter tests with relocatable Python installations; it is
not needed to import the extension. Re-run `maturin develop --uv --release` after Rust changes.
The tests use the repository's existing ebook fixtures, which are not shipped
in the source distribution.

To build a wheel and verify the source distribution can rebuild it:

```sh
maturin build --release --locked --sdist --out dist
```

Local Linux wheels are not necessarily portable. Release Linux wheels must be
built in a manylinux container or with a compatible Zig toolchain.

## Packaging and support

- Distribution: `boko-ebook`; Python import: `boko`. The distribution name is a
  release candidate, not reserved. It conflicts at import time with the unrelated
  PyPI distribution named `boko` if both are installed.
- Version and project metadata come from `Cargo.toml`; the GPL-3.0-or-later license
  and `LICENSE` file are included in the distribution.
- The `abi3-py311` feature produces one wheel per OS/architecture for regular
  CPython 3.11+. Free-threaded Python, PyPy, and GraalPy wheels are not included
  in the draft release matrix.
- Source installations require Rust 1.91+ and the platform's native build tools.
  Prebuilt wheels do not require Rust.
- `boko.pyi` supplies the API types; maturin includes it and a `py.typed` marker.
- Maturin sets `PYO3_BUILD_EXTENSION_MODULE`; do not enable PyO3's historical
  `extension-module` feature globally, which can break Rust test linking.

## Release setup

`.github/workflows/python.yml` builds five native wheel targets, tests each on
CPython 3.11 and 3.15, rebuilds/tests the source distribution, and publishes only
on `v<version>` tags matching `Cargo.toml`. It is separate from the generated
cargo-dist workflow. Branch pushes, pull requests, and manual runs do not publish.

Before the first release:

1. Confirm the distribution name and the decision to use `import boko`.
2. Configure the PyPI project/pending trusted publisher for the repository,
   workflow `python.yml`, and GitHub environment `pypi`.
3. Create that GitHub environment with required reviewers and tag restrictions.
4. Rehearse on TestPyPI using the built artifacts. TestPyPI requires its own
   publisher configuration; the draft workflow targets production PyPI only.
5. Review the wheel contents and cross-platform CI results before approving
   the production publishing job. No long-lived PyPI token is needed.

For a manual TestPyPI rehearsal with credentials configured externally:

```sh
maturin upload --repository-url https://test.pypi.org/legacy/ dist/*
```

A persistent `Book` class, chapter/asset access, section-tree extraction,
optimization, and free-threaded wheels are deliberately outside this draft.
