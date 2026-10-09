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

## Thử nghiệm Docling.rs trên Windows, 2026-10-08

Đã build worker Rust riêng từ Docling.rs `v1.104.2`, commit `29de9d1e842e6ebb35890c3f171ac0c38f273eed`, bằng toolchain Windows GNU và chế độ `ort-load-dynamic`. Worker dùng ONNX Runtime DLL 1.30.0 đã có trong runtime RapidOCR, không gọi Python để chạy Docling. Python chỉ điều phối benchmark và chạy baseline RapidOCR. Chưa tích hợp engine này vào ứng dụng/Settings.

[Báo cáo tiếng Việt có ảnh và khung tọa độ](../reports/docling-rs-2026-10-08/comparison.html), [JSON đầy đủ của các lượt đo](../reports/docling-rs-2026-10-08/comparison.json).

Model tối thiểu: Heron INT8, PP-OCRv6 detector/recognizer và dictionary, tổng 99.934.245 byte (95,3 MiB). Các file được kiểm tra SHA-256 cố định trong script build. Bản worker tại thời điểm benchmark khoảng 38,9 MiB; DLL ONNX khoảng 17,6 MiB. Đây là kích thước thành phần thử nghiệm, chưa phải mức tăng của installer. Không tải TableFormer, audio hoặc model enrichment.

Đo tuần tự 8 ảnh/biến thể × 2 engine × 6 lượt = 96 lượt. Mỗi ảnh bỏ lượt đầu, lấy median và P95 quan sát của 5 lượt warm; chưa đủ mẫu để đại diện P95 sản xuất. Docling giữ pipeline trong RAM. Baseline gọi worker RapidOCR Mobile hiện tại, sau đó gọi chính `layout.rs` của workspace qua một command riêng trong worker thử nghiệm. Mỗi engine dùng giới hạn 4 thread; IPC và decode ảnh nằm trong thời gian đo. Tiền xử lý và model của hai pipeline khác nhau; đây là so sánh hành vi ứng dụng, không phải so sánh tốc độ inference cùng model.

| Ảnh | Docling warm median / P95 | Rapid Mobile warm median / P95 | Số block Docling / Rapid |
| --- | ---: | ---: | ---: |
| Textract gốc | 1996 / 2054 ms | 878 / 1083 ms | 5 / 5 |
| Thẻ Nhật | 1012 / 1086 ms | 378 / 409 ms | 3 / 3 |
| Chữ Nhật nhỏ | 1051 / 1105 ms | 319 / 353 ms | 5 / 2 |
| Danh sách Nhật | 967 / 981 ms | 401 / 449 ms | 5 / 5 |
| Kết quả tìm kiếm | 2091 / 2191 ms | 729 / 766 ms | 5 / 6 |
| Textract scale 75% | 1880 / 1887 ms | 796 / 837 ms | 5 / 4 |
| Textract scale 150% | 2195 / 2246 ms | 884 / 947 ms | 5 / 6 |
| Textract crop | 2001 / 2069 ms | 813 / 816 ms | 5 / 5 |

Docling giữ đúng một tiêu đề và bốn mục Textract trên cả bốn biến thể. Tuy nhiên, chưa đạt cổng tích hợp:

- Thẻ Nhật: JSON gốc chứa `<!-- formula-not-decoded -->` không có provenance, thay vì tiêu đề. Adapter ghi vào `skipped`; không tự tạo tọa độ cho nội dung thiếu. Hai mục đầu cũng bị gộp. Số block bằng baseline không đồng nghĩa bố cục đúng.
- Kết quả tìm kiếm: JSON gốc gộp website/URL thứ hai vào mô tả thứ nhất, nhưng bbox của đoạn đó chỉ bao mô tả đầu. Đây là sai lệch giữa text và provenance ngay trước adapter; đưa trực tiếp vào renderer có thể bỏ sót chữ gốc hoặc vẽ nhầm vùng. Nhãn metadata của OverText cũng chưa được bảo toàn.
- P95 quan sát vượt mục tiêu 2 giây ở một số ảnh. Peak working set trong toàn lần đo khoảng 1387 MiB cho worker Docling, 526 MiB cho worker Rapid. Đây là RAM riêng từng process, không phải RAM tổng của ứng dụng; peak có tính tích lũy qua các ảnh. Worker Docling còn chứa adapter `layout.rs` dùng cho baseline.
- Lỗi ký tự tăng trên thẻ Nhật, chữ Nhật nhỏ và ảnh tìm kiếm, dù cải thiện một số ảnh khác. Các lượt dùng `ch` cho ảnh Nhật chỉ để khảo sát model đa ngôn ngữ: API `OcrLang` của bản pin chỉ chấp nhận English/Chinese, không chấp nhận `ja`.

Adapter prototype duyệt cây `body` để giữ thứ tự đọc, chuyển cả `TOPLEFT` và `BOTTOMLEFT` về pixel ảnh, kiểm tra bbox hữu hạn/trong ảnh, và giữ nguyên các vùng đã gom của Docling. Không chạy lại heuristic gom dòng trên vùng Docling. `lineHeight`/`lineCount` hiện được ước lượng từ projection mực chữ để chẩn đoán; chưa phải hợp đồng font cho renderer. Marker được giữ thành trường riêng. Chưa đo dịch thật, xóa chữ, clipping hoặc font của ảnh dịch; không coi ảnh khung đỏ là bằng chứng renderer đã được sửa.

Kết luận: giữ prototype tách biệt và RapidOCR Mobile hiện tại. Hướng nghiên cứu tiếp là dùng layout model làm tín hiệu cấu trúc, đối chiếu với toàn bộ dòng OCR để kiểm tra độ phủ trước khi chấp nhận vùng, giữ website/code theo hợp đồng hiện có, và tách vùng xóa chữ khỏi vùng vẽ bản dịch. Chỉ dùng kết quả model trực tiếp là chưa đủ để thay heuristic an toàn.

Tái lập với Cargo trên PATH và Python của runtime RapidOCR đã cài (không thay Python hệ thống):

```powershell
./scripts/build-docling-rs-prototype.ps1
& "$env:LOCALAPPDATA/OverText/rapidocr/python.exe" scripts/benchmark-docling-rs.py
$env:OVERTEXT_DOCLING_INTEGRATION = '1'
& "$env:LOCALAPPDATA/OverText/rapidocr/python.exe" scripts/test-docling-rs.py
```

Máy này dùng Cargo/Rustup/w64devkit tại `$env:TEMP/overtext-windows-tools`; cần thêm các thư mục `bin` tương ứng vào PATH và đặt `RUSTFLAGS=-C link-self-contained=yes` khi dùng GNU. Source, Cargo output, model và graph cache nằm trong `logs/` đã được ignore. Benchmark không tự tải model. Worker báo lỗi khi thiếu model/runtime, không fallback sang engine khác. Giao thức NDJSON có request ID, lỗi từng request và lệnh shutdown; supervisor benchmark có timeout 120 giây và dừng process lỗi, chưa có cơ chế restart cho tích hợp production. Event `ready` chỉ báo giao thức sẵn sàng, chưa warm model.

Kiểm chứng: build release worker thành công; 5 kiểm thử adapter và 1 kiểm thử worker thật đều pass. Kiểm thử worker xác nhận yêu cầu ngôn ngữ không hỗ trợ trả lỗi nhưng request tiếp theo vẫn nhận diện Textract thành công. Cổng layout và hiệu năng là kết quả benchmark riêng, không bị coi là pass chỉ vì unit test pass. Các file mới dùng UTF-8 và LF.

Nguồn đối chiếu: [Docling.rs tại commit pin](https://github.com/docling-project/docling.rs/tree/29de9d1e842e6ebb35890c3f171ac0c38f273eed), [API OCR của bản pin](https://github.com/docling-project/docling.rs/blob/29de9d1e842e6ebb35890c3f171ac0c38f273eed/crates/docling-pdf/src/ocr.rs), [model release](https://github.com/docling-project/docling.rs/releases/tag/models-v1). Số đo và lỗi cụ thể ở trên là kết quả local, không phải benchmark công bố của upstream.

## Corpus kiểm thử Windows OCR và RapidOCR — 2026-10-09

Đã chạy cùng pipeline OCR và `layout.rs` của ứng dụng cho Windows OCR, RapidOCR Mobile và RapidOCR Server: 14 ảnh × 3 engine × 6 lượt = **252 lượt**, không có request lỗi. Windows đã cài pack `en-US` và `ja`; RapidOCR 3.9.2 chạy ONNX Runtime 1.30.0, CPU bốn thread. Không thay engine mặc định hay runtime ứng dụng.

Xem [báo cáo OCR/layout kèm ảnh đối chiếu](../reports/ocr-corpus-2026-10-09/summary.html), [báo cáo renderer](../reports/ocr-corpus-2026-10-09/renderer.html), [kết quả thô](../reports/ocr-corpus-2026-10-09/raw-results.json) và [manifest corpus](../reports/ocr-corpus-2026-10-09/corpus-manifest.json).

Corpus gồm 8 screenshot local: Textract gốc, thu nhỏ 75%, phóng 150%, crop; ba ảnh Nhật và ảnh kết quả tìm kiếm. Text và vùng đoạn tham chiếu được gán trước khi chấm engine. Thêm 3 trang slide tiếng Anh từ OmniDocBench và 3 trang slide Nhật từ JASyn, chọn theo tên file và điều kiện nội dung cố định, không chọn theo điểm OCR. Hai trang Nhật cùng một tài liệu; mẫu này chưa đại diện cho mọi ngôn ngữ hay loại tài liệu. Chưa chấm bảng/công thức và chưa mở rộng sang tiếng Việt hoặc ngôn ngữ khác.

Dataset được pin revision, annotation và từng ảnh có SHA-256. JASyn chỉ có nhãn block, không dùng nó để giả lập ground truth dòng. Header không có nhãn reading order được giữ khi chấm text nhưng bỏ khỏi cặp thứ tự. Các ảnh số liệu dùng PNG đầy đủ; JPEG trong HTML chỉ là preview.

### Kết quả evaluator chính thức

Dùng source OmniDocBench pin `f133a71e9e91c3621c7ce8994200a7b394a06eb3`, Python 3.10.11 trong môi trường riêng, dependency gốc; phiên bản cụ thể lưu trong `evaluator-requirements.txt`. Export block ứng dụng sang Markdown và chạy `quick_match` với `text_block.Edit_dist`, `reading_order.Edit_dist`: 6 cấu hình đều hoàn thành, không timeout/fallback. Dùng `quick_match` vì `simple_match` của revision này lỗi kiểu dữ liệu trong adapter cross-category; không sửa code evaluator. Các lỗi tách/gộp bị matcher dung thứ vẫn được chấm riêng theo vùng ở báo cáo local.

| Tập con | Engine | Text Edit_dist | Reading order Edit_dist |
| --- | --- | ---: | ---: |
| Anh, 3 trang | Windows | 0.0033 | 0 |
| Anh, 3 trang | Rapid Mobile | 0.0019 | 0 |
| Anh, 3 trang | Rapid Server | 0 | 0 |
| Nhật, 3 trang | Windows | 0.1417 | 0 |
| Nhật, 3 trang | Rapid Mobile | 0.0990 | 0.1333 |
| Nhật, 3 trang | Rapid Server | 0.1127 | 0.1333 |

Điểm thấp tốt hơn. Đây là evaluator chính thức chạy trên **tập con**, không phải điểm benchmark đầy đủ. Không so sánh trực tiếp với CER local: CER local chuẩn hóa NFKC, bỏ dấu câu/khoảng trắng và so từng vùng nguồn; evaluator chính thức có normalization và matching riêng. Rapid Mobile đọc chữ Nhật tốt hơn trên mẫu này nhưng có lỗi thứ tự đọc; Server không tốt hơn Mobile ở mọi ảnh.

### Tách lỗi OCR, gom đoạn và renderer

`scripts/ocr-corpus.py` đo CER theo vùng, vùng thiếu, vùng bị tách, block gộp nhiều vùng và cặp thứ tự bị đảo. Vùng có dòng OCR không có nghĩa toàn bộ chữ đúng. Các dòng ngoài vùng tham chiếu được báo riêng; không âm thầm tính chúng là chữ đúng. Mỗi lượt đều được chấm, báo cáo kiểm tra điểm có ổn định qua sáu lượt hay không.

Textract gốc cho thấy sự khác biệt: Windows sai 6/477 ký tự chuẩn hóa và tách mục đầu; Mobile sai 5/477 và không tách/gộp; Server sai 4/477 nhưng gộp sai một block. Vì vậy chọn engine bằng CER đơn lẻ sẽ bỏ qua lỗi cấu trúc mà người dùng đang gặp.

Renderer được chạy với bbox thật của mỗi engine: 8 ảnh local × 3 engine × scale 1 và 1.5 = **48 probe**. Font được pin; text Việt tạo có kiểm soát để đo fit, không phải bản dịch từ provider. Probe xóa chữ mà chưa vẽ bản dịch để đo mực nguồn còn sót trong vùng độc lập, kiểm tra pixel metadata cần giữ và glyph vượt clip.

| Engine | Probe có pixel mực còn sót | Probe thay đổi metadata | Lượt vẽ vượt clip |
| --- | ---: | ---: | ---: |
| Windows | 12/16 | 2/16 | 2 |
| Rapid Mobile | 2/16 | 0/16 | 0 |
| Rapid Server | 2/16 | 0/16 | 0 |

Đếm mực còn sót dùng ngưỡng màu cách nền trắng >100 và ngưỡng số pixel >0; một pixel cũng được ghi nhận, **không coi mọi số khác 0 là lỗi nhìn thấy nghiêm trọng**. Cần xem số pixel và ảnh trong HTML để phân biệt viền antialias với chữ còn nguyên. Probe chỉ dùng ảnh nền trắng local, không áp dụng cách đo này cho nền màu/phức tạp. Điểm 0 không chứng minh bản dịch đúng hoặc font đẹp. Các chẩn đoán hiện tại được lưu trung thực, không đổi fixture để làm chúng pass.

Thời gian OCR được ghi riêng lượt đầu và median/P95 năm lượt sau. Đây là phép đo thăm dò trên desktop, chưa kiểm soát tải nền; không dùng để xếp hạng tốc độ chính xác. Các engine gọi tuần tự, nhưng trong đợt chạy có tác vụ build/test nền. Lượt đầu cũng không đồng nghĩa khởi động lạnh có kiểm soát.

### Chạy lại và hướng xử lý

Cần runtime RapidOCR và cả hai bộ model đã cài, Windows có pack Anh/Nhật, Node dependency và Cargo khả dụng. Benchmark không tự fallback khi engine lỗi. Chuẩn bị/chạy lại:

```powershell
./scripts/run-ocr-corpus.ps1 -ReportDirectory reports/ocr-corpus-2026-10-09
./scripts/install-omnidocbench-eval.ps1
./scripts/run-omnidocbench-eval.ps1 -ReportDirectory reports/ocr-corpus-2026-10-09
# Tổng hợp lại từ raw đã đo; không chạy lại OCR:
./scripts/run-ocr-corpus.ps1 -SkipInference -ReportDirectory reports/ocr-corpus-2026-10-09
& "$env:LOCALAPPDATA/OverText/rapidocr/python.exe" scripts/test-ocr-corpus.py
```

Corpus/runtime/evaluator/model tải về nằm trong `logs/` đã ignore, không nằm trong bộ cài ứng dụng. YAML evaluator chứa đường dẫn tuyệt đối; chạy lại scoring để tái tạo sau khi chuyển workspace. Report lưu kết quả và preview, ảnh PNG đầy đủ tái lập bằng bước prepare. `-Official` chạy thêm evaluator đã cài; bộ đầy đủ dùng ba engine mặc định.

Điểm evaluator chỉ được đưa vào HTML khi checksum ground truth và Markdown còn khớp đầu vào đã chấm; đổi kết quả OCR cần chạy lại evaluator. Kiểm chứng local: 7 test chấm điểm mới, 51 test Rust desktop và 60 test Playwright đạt; Clippy toàn bộ target test sạch. Sáu test Rust cần runtime/model được opt-in riêng, trong đó benchmark corpus đã chạy đủ 252 request ở trên. Các file mới dùng UTF-8 không BOM và LF.

Bảy unit test chấm điểm kiểm tra text thiếu, tách/gộp, thứ tự đọc, fragment cùng dòng và nhãn nullable; benchmark OCR được opt-in qua test desktop `--ignored`. Unit test xanh chỉ xác nhận harness và quy tắc, không xóa các lỗi chất lượng được phát hiện. Với kết quả hiện tại, giữ Mobile mặc định; ưu tiên hợp đồng layout/renderer chung: giữ dòng OCR và vùng xóa nguồn độc lập với vùng đặt bản dịch, bảo toàn metadata, rồi dùng corpus kiểm tra mọi engine. Mỗi sửa đổi cần so lại cả lỗi đọc chữ, tách/gộp và pixel; chưa đủ dữ liệu để tự chuyển model theo ngôn ngữ hoặc tích hợp Docling vào production.

Nguồn và quyền sử dụng: [OmniDocBench](https://github.com/opendatalab/OmniDocBench) có code Apache 2.0 nhưng dataset chỉ dùng nghiên cứu, không thương mại; [OmniDocBench-JASyn](https://huggingface.co/datasets/stockmark/OmniDocBench-JASyn) công bố CC BY 4.0 và nhãn block, không dùng dữ liệu để train model trong công việc này. Các mẫu chỉ phục vụ đánh giá, không đóng gói trong ứng dụng.

## Tổng hợp macOS trước khi thống nhất RapidOCR Mobile

Mục tiêu tiếp theo là bổ sung bằng chứng từ macOS vào cùng corpus rồi thống nhất RapidOCR Mobile làm mặc định. Windows hiện đã mặc định Mobile trong `Settings::default`; macOS vẫn dùng Apple Vision trong `recognize_configured`, không áp dụng lựa chọn engine Windows. Chưa thay đường OCR macOS trong branch báo cáo này. Để dùng Mobile trên macOS cần bổ sung runtime/model, routing và cài đặt đúng kiến trúc máy, sau đó kiểm tra thực tế; đổi giá trị settings đơn lẻ không đủ.

Hiện chưa có lượt OCR native macOS được đo trong đợt benchmark 2026-10-09. Ảnh đối chiếu người dùng gửi và fixture Vision hiện có là tư liệu hồi quy, không thay thế benchmark macOS. Không kết luận macOS không có lỗi từ những ảnh đó.

| Nhóm cần tổng hợp | Bằng chứng cần lưu trên macOS | Trạng thái |
| --- | --- | --- |
| Đọc chữ theo ngôn ngữ | PNG gốc, text/bbox từng dòng Vision, pack/hint và phiên bản macOS | Chờ đo |
| Tách/gộp đoạn và thứ tự đọc | Dòng OCR trước layout, block sau layout, nhãn đoạn độc lập; chạy lại Textract, search, ảnh Nhật | Chờ đo |
| Retina, tọa độ, crop | Kích thước pixel ảnh, kích thước logical vùng chọn, scale màn hình, vị trí cửa sổ; thử scale 1/2 và nhiều màn hình | Chờ đo |
| Xóa chữ, font, clipping | Ảnh chỉ xóa chữ và ảnh dịch, vùng metadata, font thực tế; so cùng PNG và text kiểm thử với Windows | Chờ đo |
| Tốc độ và bộ nhớ | Lượt đầu/warm, thời gian OCR/layout/render riêng, kiến trúc CPU và tải nền | Chờ đo |
| Rapid Mobile trên macOS | Cài runtime/model, nhận diện thật trên máy đích, offline sau cài, khởi động lại và xử lý lỗi | Chưa tích hợp/đo |

Mỗi lỗi lưu một mã case, engine/OS/build, bước tái hiện, kết quả mong đợi, ảnh gốc, text/bbox OCR và block layout. Ghép các case Mac vào corpus chung để phân biệt lỗi riêng engine với lỗi layout/renderer dùng chung; không thêm heuristic dựa vào text của từng ảnh. Harness OCR hiện tại chỉ chạy native Windows, cần mở rộng target macOS; chấm lại JSON Vision đã ghi chỉ là hồi quy offline, không phải chạy engine thật.

Thứ tự xử lý sau khi có số liệu Mac:

1. Tách vùng xóa chữ nguồn từ các bbox dòng OCR khỏi vùng đặt bản dịch của block, bảo toàn website/code.
2. Giữ provenance các dòng trong block; sửa quy tắc gom đoạn bằng baseline, khoảng cách, căn lề, cột và marker, kiểm tra mọi engine trên corpus chung.
3. Chuyển các chẩn đoán ổn định thành cổng hồi quy; hiệu chỉnh ngưỡng pixel để phân biệt antialias với chữ còn sót, giữ kiểm tra metadata và clipping.
4. Đánh giá Mobile và Vision trên cùng corpus macOS; hoàn tất runtime/routing/cài đặt Mobile rồi mới đổi mặc định macOS. Giữ lựa chọn engine rõ ràng và báo lỗi model/runtime thiếu, tránh âm thầm dùng engine khác.

Điều kiện thống nhất Mobile: chạy thật trên các kiến trúc Mac được sản phẩm hỗ trợ, cài/cập nhật model thành công, corpus Windows/Mac không phát sinh hồi quy về text, cấu trúc, tọa độ hoặc metadata, và độ trễ/RAM có số đo chấp nhận được. Khi cần giữ một ngoại lệ hoặc engine thay thế, ghi bằng số liệu corpus thay vì xử lý riêng từng nội dung ảnh. Không dùng các số liệu Windows hiện tại để tuyên bố Mobile đã đạt trên macOS.
