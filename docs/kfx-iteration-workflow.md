# KFX Schema Iteration Workflow

Development workflow for testing and improving the KFX import/export pipeline.

## Tools

| Tool | Purpose |
|------|---------|
| `boko info <file>` | Metadata, spine, TOC, assets - quick sanity check |
| `boko dump <file>` | IR tree structure, styles - verify import parsing |
| `boko convert <in> <out>` | Format conversion - test full pipeline |
| `kfx-dump <file>` | Raw KFX entities/Ion - inspect binary format |

## Workflow

### 1. Baseline Capture

```bash
EPUB=tests/fixtures/epictetus.epub
boko dump "$EPUB" --styles-only > /tmp/epub-styles.txt
boko info "$EPUB" > /tmp/epub-info.txt
```

### 2. Export Test (EPUB → KFX)

```bash
boko convert "$EPUB" /tmp/test.kfx
kfx-dump /tmp/test.kfx 2>&1 | grep -A20 "Type: \$157 (style)" | head -60
boko info /tmp/test.kfx
```

### 3. Round-Trip Test (KFX → IR)

```bash
boko dump /tmp/test.kfx --styles-only > /tmp/kfx-styles.txt
diff /tmp/epub-styles.txt /tmp/kfx-styles.txt
```

### 4. Full Round-Trip (EPUB → KFX → EPUB)

```bash
boko convert /tmp/test.kfx /tmp/roundtrip.epub
boko dump /tmp/roundtrip.epub --styles-only > /tmp/roundtrip-styles.txt
diff /tmp/epub-styles.txt /tmp/roundtrip-styles.txt
```

## Adding New Style Properties

1. **Schema** (`src/kfx/style_schema.rs`):
   - Add `IrField::NewProperty` variant
   - Add schema rule with `ir_field: Some(IrField::NewProperty)`
   - Add case to `extract_ir_field()` (export)
   - Add case to `apply_ir_field()` (import)

2. **Test**:
   ```bash
   cargo test kfx
   boko convert test.epub /tmp/t.kfx && kfx-dump /tmp/t.kfx | grep "new_property"
   ```

## Quick Commands

```bash
# Full round-trip with style comparison
boko convert "$EPUB" /tmp/t.kfx && boko dump /tmp/t.kfx --styles-only

# Count style entities in KFX
kfx-dump /tmp/test.kfx 2>&1 | grep -c "Type: \$157"

# Show unique style properties
kfx-dump /tmp/test.kfx 2>&1 | grep -A30 "Type: \$157" | grep -E "^\s+\w+:" | sort -u
```
