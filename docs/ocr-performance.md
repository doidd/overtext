OCR performance comparison on the local Windows machine, 2026-10-05.

Subsequent user decision: Windows now defaults to RapidOCR Mobile to prioritize latency. Settings offers optional RapidOCR Server downloads and explicit Windows/Paddle engine selection. The measurements and quality-gate conclusions below remain historical evidence; the default change does not imply Mobile passed that gate.

Four existing PNG fixtures were read three times per configuration. The first run of each image was excluded; timings below are medians of the two warm runs. PaddleOCR used CPU, four threads, MKL-DNN enabled, the same mobile detector, and a separate worker per configuration. Cold import/model setup and model downloads are excluded from warm timings. These measurements preceded the subsequent default change.

| Fixture | Server, side 1200 (current) | Server, side 800 | Server, native pixels | Mobile, side 1200 | Windows OCR |
| --- | ---: | ---: | ---: | ---: | ---: |
| Japanese card, 347×164 | 2.93 s | 3.30 s | 2.40 s | 0.35 s | 0.25 s |
| Small Japanese text, 347×166 | 2.58 s | 3.12 s | 2.60 s | 0.31 s | 0.26 s |
| Japanese list, 408×204 | 3.50 s | 3.28 s | 3.00 s | 0.43 s | 0.35 s |
| Search results, 707×280 | 9.00 s | 5.46 s | 5.62 s | 0.74 s | 0.43 s |

The target side does not force a fixed size: upscaling is capped at 3×, and larger images are never downscaled. Windows OCR used installed `ja`/`en-US` language packs and the application's existing native preprocessing. This comparison measures complete native calls versus the Python worker's reported OCR time, not an identical low-level implementation.

Quality was scored against manually transcribed text in `scripts/benchmark-ocr-resize.py`. NFKC, whitespace and punctuation normalization remove typography differences, isolated number badges are excluded, and rows are ordered by image coordinates before comparison. Scores below are character edit counts (lower is better); they were identical across the three repeats of each configuration. These are fixture-specific counts, not a general OCR accuracy rating.

| Fixture | Server 1200 | Server 800 | Server native | Mobile 1200 | Windows |
| --- | ---: | ---: | ---: | ---: | ---: |
| Japanese card | 1 | 3 | 15 | 5 | 2 |
| Small Japanese text | 6 | 7 | 17 | 4 | 22 |
| Japanese list | 1 | 2 | 16 | 2 | 2 |
| Search results | 2 | 2 | 2 | 1 | 3 |

The acceptance gate requires no increase in character edits versus the current server configuration, and preservation of important phrases in every repeat. Lower upscaling passes for search results but damages Japanese text. The mobile recognizer is 8–12× faster here and improves the English fixture, but changes Japanese characters such as `タ` to `夕` and loses required phrases. Windows OCR is 10–21× faster here, but misreads `共通指摘ID` and performs worse on the smallest Japanese fixture. None passes the all-image gate. The current configuration itself has recognition errors; it is a comparison baseline, not perfect ground truth.

Initial quality-gate recommendation: retain the server/1200 automatic default. Do not infer screenshot language from UI locale or silently switch all Japanese captures to the faster engines. A future speed/accuracy option should be explicit, or an adaptive policy must first demonstrate reliable quality gating on a larger corpus. When the user explicitly chooses a source language and its Windows pack is installed, the existing application already uses Windows OCR; that path trades some accuracy on these fixtures for latency.

The official [PP-OCRv5 mobile model card](https://huggingface.co/PaddlePaddle/PP-OCRv5_mobile_rec/blob/main/README.md) documents Chinese, English and Japanese support. Language coverage alone does not establish accuracy on small screenshots; the measurements above decide whether it can replace the server model.

Reproduce on Windows with the installed OverText PaddleOCR runtime:

```sh
python scripts/test-paddleocr-worker.py
python scripts/benchmark-ocr-resize.py
python scripts/benchmark-ocr-resize.py --model mobile --targets 1200
cargo test --manifest-path src-tauri/Cargo.toml --test desktop benchmark_native_ocr_on_layout_fixtures --locked -- --ignored --nocapture
python scripts/benchmark-ocr-resize.py --native-report
```

Full replies, normalized boxes, stage timings and quality summaries are retained in ignored `logs/ocr-resize-comparison.json`, `logs/ocr-mobile-comparison.json`, `logs/ocr-native-comparison.json` and `logs/ocr-native-quality.json`. `--rescore` recomputes Paddle quality from saved replies without running models. The worker accepts benchmark overrides `OVERTEXT_OCR_UPSCALE_SIDE` (0–1600) and `OVERTEXT_OCR_MULTILINGUAL_MODEL` (`PP-OCRv5_server_rec` or `PP-OCRv5_mobile_rec`); unset variables retain the previous defaults. Unit tests protect the defaults, scale cap, normalized geometry and quality scoring. These overrides are diagnostic controls, not new UI settings.

Limits: four fixtures and two warm samples cannot establish performance or accuracy for all screenshots. Native OCR row order differed on one fixture, which is why scoring sorts by geometry. Layout correctness still needs the existing Rust and browser regressions; the content benchmark alone does not certify block grouping, overlay coverage or multi-monitor DPI.

RapidOCR comparison added in the same session (24 additional recorded reads): RapidOCR 3.9.2, ONNX Runtime 1.30.0 CPU, intra-op threads 4/inter-op threads 1, PP-OCRv5 mobile detector, server/mobile recognizers, 1200 upscale policy and 0.35 text-score threshold. Orientation classification is disabled. This tests explicitly selected v5 ONNX models from the [official model list](https://rapidai.github.io/RapidOCRDocs/latest/model_list/), not every RapidOCR configuration. Conversion and preprocessing/postprocessing differ from Paddle, so equal model-family names do not guarantee equal outputs.

| Fixture | RapidOCR Server warm | Server edits | Server quality gate | RapidOCR Mobile warm | Mobile edits | Mobile quality gate |
| --- | ---: | ---: | --- | ---: | ---: | --- |
| Japanese card | 3.39 s | 3 | FAIL | 0.38 s | 4 | FAIL |
| Small Japanese text | 5.00 s | 5 | PASS | 0.40 s | 3 | FAIL |
| Japanese list | 1.36 s | 0 | PASS | 0.42 s | 1 | FAIL |
| Search results | 3.14 s | 2 | PASS | 0.82 s | 1 | PASS |

The server ONNX path passes the quality gate on three images, but has more edits on the card and is slower on the smallest Japanese image. Mobile ONNX is fast but fails required Japanese phrases. Neither passes the all-image gate, so application defaults remain unchanged. Full repeats, stage timings, parameters, package versions and model SHA-256 values are in `logs/ocr-rapidocr-comparison.json` and the Vietnamese report's `du-lieu` directory. Engine runs were sequential; background load/power state were not controlled, so the small sample is not a general backend speed ranking.

Install benchmark-only dependencies separately with the existing Python 3.12 runtime (use that same interpreter for both commands):

```sh
python -m pip install --target logs/rapidocr-deps rapidocr==3.9.2 onnxruntime==1.30.0
python scripts/benchmark-rapidocr.py
```

The HTML report and ZIP now include seven configurations, four original PNGs, six JSON evidence files and 84 recorded OCR calls. They embed the screenshots and preserve earlier measurements. Dependency/model downloads are excluded from warm medians; models stay in ignored `logs/rapidocr-models` and are not packaged into the report ZIP.
