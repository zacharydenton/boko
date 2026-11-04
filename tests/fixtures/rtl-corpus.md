# External RTL regression corpus

These EPUBs are downloaded separately, not bundled in the repository.
Project Gutenberg lists all five as public domain in the USA. Their included
Gutenberg licenses remain with the downloads. Source URLs, download URLs,
file sizes, and SHA-256 hashes for the tested copies are in `rtl-corpus.json`.

| Language | Book | Source |
| --- | --- | --- |
| Arabic | Tribute to Michael Hart — Majid AlHydar | [Gutenberg 43007](https://www.gutenberg.org/ebooks/43007) |
| Hebrew | Hunger: Book One — Knut Hamsun, translated by Elhanan Segal | [Gutenberg 18291](https://www.gutenberg.org/ebooks/18291) |
| Hebrew | Tsel Ha'Olam — Gautier of Metz, translated by Mattathias Delacrut | [Gutenberg 61024](https://www.gutenberg.org/ebooks/61024) |
| Hebrew | HaTsofe leBet Israel / Tashlikh — Isaac Erter | [Gutenberg 45252](https://www.gutenberg.org/ebooks/45252) |
| Persian | Five Selected Short Stories — D. H. Lawrence, translated by Siaavash Malekei | [Gutenberg 46740](https://www.gutenberg.org/ebooks/46740) |

The Hebrew books declare RTL in their content but omit spine direction. The
Persian book also contains explicit LTR passages. The Arabic book exercises
the language fallback. These are structural regression checks, not Kindle
rendering or shaping tests.

Download from the repository root (Python standard library only):

```sh
python3 - <<'PY'
import hashlib, json, pathlib, urllib.request
root = pathlib.Path.home() / "books" / "rtl-regression"
root.mkdir(parents=True, exist_ok=True)
for entry in json.loads(pathlib.Path("tests/fixtures/rtl-corpus.json").read_text()):
    path = root / entry["file"]
    data = path.read_bytes() if path.exists() else urllib.request.urlopen(entry["download"], timeout=60).read()
    assert hashlib.sha256(data).hexdigest() == entry["sha256"], f"Source changed: {path.name}; review before updating the recorded hash"
    path.write_bytes(data)
PY
```

Run the opt-in corpus test:

```sh
BOKO_RTL_CORPUS="$HOME/books/rtl-regression" \
  cargo test --test rtl_direction open_rtl_books -- --ignored --nocapture
```

Optionally set `BOKO_RTL_OUTPUT` to save converted files and text snapshots.
The test checks EPUB → KFX/AZW3 → EPUB → KFX/AZW3 page direction and compares
the first EPUB round trip's text and per-character base direction with the
source. It ignores layout whitespace and zero-width spaces (existing KFX
anchor markers), while retaining bidi controls, combining marks, ZWNJ, and ZWJ.
Synthetic tests in `rtl_direction.rs` additionally cover explicit LTR page
order, script subtags, CSS-wide direction keywords, and logical/physical
alignment at direction changes.
