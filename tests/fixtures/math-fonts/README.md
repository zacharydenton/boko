# Math font regression fixtures

These are renamed, unhinted subsets used by the math font, layout, and KVG
emission tests. Tests must not depend on fonts installed on the host.

- `boko-math-cff.otf`: STIX Two Math 2.0.2, CFF outlines, 168 glyphs.
  [Original font](https://github.com/stipub/stixfonts/blob/master/archive/STIXv2.0.2/OTF/STIX2Math.otf).
  The accompanying `STIX-LICENSE.txt` is extracted from the upstream
  [2.0.2 license](https://github.com/stipub/stixfonts/blob/master/archive/STIXv2.0.2/docs/STIX_2.0.2_license.pdf).
- `boko-math-glyf.ttf`: DejaVu Math TeX Gyre 2.37, TrueType outlines,
  158 glyphs, including the composite `é`.
  [Upstream](https://github.com/dejavu-fonts/dejavu-fonts).
  See `DejaVu-LICENSE.txt` for the original license.

Created with fontTools 4.68.0 using `fontTools.subset.Subsetter`, with
`hinting=False`, `name_IDs=['*']`, `name_legacy=True`, and
`name_languages=['*']`. All other subset options use their defaults. The
input characters are:

```text
 abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789+-=()[]∑∫√→⏞αβé𝑥𝑎𝑏𝑐𝑑𝑖𝑛
```

The leading space is intentional. MATH variants and assembly parts are
retained by the subsetter. Name IDs 1, 3, 4, 16, and 17 were renamed to
`Boko Math CFF Fixture` / `Boko Math Glyf Fixture`; name ID 6 uses the same
name without spaces. The CFF font name, FamilyName, and FullName were also
renamed. Original copyright, trademark, and license entries remain intact.

The adjacent `.txt` baselines were captured from the previous adapter using
`ttf-parser` 0.25.1, before removing that dependency. They record units per
em, all consumed MATH constants, and each glyph's metrics, outline SHA-1,
and vertical/horizontal variant choices at 0, 500, 1000, 2000, and 10000
font units. Character mappings, including a missing character, follow.
Outline hashes cover sorted directed segments with explicit closing edges:
this allows equivalent contour starting points and closing-line encodings
while checking all coordinates and winding directions. SHA-1 is only a
regression checksum here.

The migration was additionally compared against all 5,543 glyphs in the
full STIX font and all 4,282 glyphs in the full DejaVu font. Metrics, MATH
constants, variant choices, and normalized outlines matched exactly.
