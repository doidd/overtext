Layout decisions use a fixed priority order. Sentence continuation may relax OCR box-height variation, but cannot override a boundary established earlier.

| Priority | Rule | Effect |
| --- | --- | --- |
| 1 | An intervening block overlaps the candidate's column | Do not jump back across it. Interleaved, separate columns may still join. |
| 2 | Maximum block length, code/prose transition, or a new list marker | Start another block. Keep code indentation and syntax colors independent of prose rules. |
| 3 | Website/URL metadata, heading, and body roles differ | Start another block. A URL mentioned in a sentence is body text, not metadata. |
| 4 | Strong foreground change on a similar background, without body-continuation evidence | Separate independent prose styles. Color alone is ambiguous within an unfinished wrapped sentence; preserve colored inline-code continuations. Ignore this check for code highlighting and metadata. |
| 5 | Vertical spacing or alignment is incompatible | Start another block, regardless of punctuation. |
| 6 | An unfinished, sufficiently wide body row continues | Allow the existing 0.45–1.8 height range in body text only. This preserves short inline-code boxes and taller rows within paragraphs. |
| 7 | Otherwise | Require the ordinary 0.75–1.33 height range. |

Heading hints are computed before grouping. They require an independent row, nearby smaller body text, and corroboration from a metadata predecessor, changed style, or a large, shorter title. A smaller/differently colored row followed by a return to the original style within the same sentence is an inline variation, not heading evidence. Matching wrapped title rows inherit the hint; smaller body rows do not. Bounding-box height alone is not a reliable font measurement.

Standalone metadata keeps its original text and line breaks. The translation pipeline excludes metadata and code, and the image renderer preserves their original pixels.

Both OCR engines feed this same policy. Thresholds are relative to OCR line height rather than fixed Windows/macOS pixel sizes. Debug builds log accepted joins as well as rejected joins, including roles, spacing, height ratio and color distance; source text is excluded from these runtime logs.

| Regression | Required result |
| --- | --- |
| Real PaddleOCR search-results fixture | Six blocks: metadata/title/body for each result; separate title and snippet colors |
| Real Vision paragraph with 13 rows | One paragraph despite varying OCR box heights |
| Taller inline row / short or colored inline-code row | Continue the paragraph |
| URL mentioned inside prose, including at the start of a sentence | Keep it in the translated paragraph |
| Wrapped heading | Join title rows, then split before body text |
| Japanese bullets without spaces | Keep four separate list items |
| Indented code | Preserve the code block and line breaks |
| Interleaved columns | Join each column separately |
| A link inserted between two body rows | Never bypass the link to join the older paragraph |

Run `cargo test --manifest-path src-tauri/Cargo.toml --test desktop --locked` and `node --test scripts/test-translation.mjs` when changing these rules. The PNG and recorded OCR boxes in `src-tauri/assets/ocr-search-results.*` make this regression independent of model downloads and OCR engine updates. The rules remain heuristics; new ambiguous layouts should be added as fixtures rather than addressed by widening continuation thresholds globally.

Browser render regressions run with `npx playwright install chromium` followed by `npm run test:render`. CI runs them on Windows and macOS, following Playwright's browser installation workflow (https://playwright.dev/docs/ci). The production Canvas renderer uses the real search-results and Japanese-list PNGs with recorded block geometry and fixed Vietnamese translations; no OCR model or translation provider is contacted. These fixtures isolate rendering from grouping; Rust tests separately protect grouping decisions. Review/update recorded blocks if the layout contract deliberately changes.

For both images at device scales 1, 1.25, 1.5, and 2, tests assert that source block interiors are completely erased, translated blocks contain visible ink, metadata stays pixel-identical even if given a changed translation, and surroundings (including the Japanese badge/border) stay unchanged. Captured draw operations must include every translated character, retain block colors, fit within allowed regions, and center the complete paragraph. A two-pixel optical-centering tolerance allows accents to differ between rows. Tests attach output PNGs; CI uploads test-results on failure. Pixel expectations are calculated within the same browser rather than using platform-sensitive font screenshots.

This covers image-coordinate rendering, not OS capture scaling, monitor placement, clipboard/save dialogs, missed OCR glyphs outside recorded boxes, or arbitrarily long translations that cannot fit at the renderer's minimum font size. Those require additional fixtures or end-to-end/manual checks; device-scale emulation does not certify native Windows DPI conversion.

## Cross-platform render tests

Canvas text measurement depends on the installed font, so identical input can wrap
differently on macOS and Windows. `tests/render/render.spec.ts` therefore runs three modes:

- `pinned-font`: the bundled Noto Sans (`tests/render/fonts`); identical on every OS and the
  reference result. Linux CI runs it as well.
- `system-font`: the production font stack on the host OS.
- `stress-long`: pinned font with ~50% longer translations; the pixel-containment
  invariants must still hold (nothing outside the allowed regions, source fully erased).

`renderTranslatedImage(src, translation, { fontFamily })` accepts the font so tests can pin it.
