"""Compare resident Docling.rs and production RapidOCR, without changing app defaults.

Run with the installed RapidOCR Python (OpenCV/numpy are already installed).
Reports include original images and geometry overlays; no translation API is used.
"""
import argparse
import base64
import hashlib
import html
import importlib.util
import json
import math
import os
from pathlib import Path
import queue
import statistics
import subprocess
import sys
import threading
import time

ROOT = Path(__file__).resolve().parents[1]


class Worker:
    def __init__(self, command, env, log):
        self.log = open(log, "w", encoding="utf-8")
        self.process = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                        stderr=self.log, text=True, encoding="utf-8", env=env,
                                        cwd=ROOT, creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0))
        self.replies = queue.Queue()
        self.sequence = 0
        def read():
            for line in self.process.stdout:
                self.replies.put(line)
            self.replies.put(None)
        self.reader = threading.Thread(target=read, daemon=True)
        self.reader.start()

    def receive(self):
        try:
            line = self.replies.get(timeout=120)
        except queue.Empty:
            self.close()
            raise TimeoutError("Worker exceeded 120 seconds; see stderr log")
        if line is None:
            raise RuntimeError(f"Worker exited ({self.process.poll()}); see stderr log")
        return json.loads(line)

    def request(self, data):
        self.sequence += 1
        data = dict(data, id=self.sequence, request_id=self.sequence)
        started = time.perf_counter()
        self.process.stdin.write(json.dumps(data, ensure_ascii=False) + "\n")
        self.process.stdin.flush()
        reply = self.receive()
        if "id" in reply and reply["id"] != self.sequence:
            raise RuntimeError("Worker response ID mismatch")
        elapsed = (time.perf_counter() - started) * 1000
        if reply.get("error"):
            raise RuntimeError(reply["error"])
        return reply, elapsed

    def close(self):
        if self.process.poll() is None:
            self.process.kill()
        self.process.wait(timeout=10)
        self.process.stdout.close()
        self.process.stdin.close()
        self.reader.join(timeout=1)
        self.log.close()


def ordered_texts(document):
    """Follow the body tree, rather than assuming the texts array is reading order."""
    seen = set()
    def visit(item):
        if "$ref" in item:
            ref = item["$ref"]
            if ref in seen:
                raise ValueError(f"Repeated/cyclic document reference: {ref}")
            seen.add(ref)
            parts = ref.removeprefix("#/").split("/")
            item = document
            for part in parts:
                item = item[int(part)] if isinstance(item, list) else item[part]
        if "text" in item:
            yield item
        for child in item.get("children", []):
            yield from visit(child)
    return list(visit(document["body"]))


def normalize_box(provenance, width, height):
    if provenance.get("page_no") != 1:
        raise ValueError("Screenshot adapter accepts only page 1")
    box = provenance["bbox"]
    left, right = float(box["l"]), float(box["r"])
    top, bottom = float(box["t"]), float(box["b"])
    origin = box.get("coord_origin")
    if origin == "BOTTOMLEFT":
        top, bottom = height - top, height - bottom
    elif origin != "TOPLEFT":
        raise ValueError(f"Unknown coordinate origin: {origin}")
    if not all(math.isfinite(v) for v in (left, right, top, bottom)):
        raise ValueError("Non-finite provenance geometry")
    if right <= left or bottom <= top:
        raise ValueError("Reversed/empty provenance geometry")
    if left < -1 or top < -1 or right > width + 1 or bottom > height + 1:
        raise ValueError("Provenance outside screenshot dimensions")
    left, top, right, bottom = max(0, left), max(0, top), min(width, right), min(height, bottom)
    return left, top, right - left, bottom - top


def adapt_document(reply, image):
    """Prototype renderer adapter. Never re-merge Docling's grouped text regions.

    Docling JSON has region boxes but no OCR row heights. Ink projections below
    are diagnostic estimates, explicitly not a production font-size contract.
    """
    import cv2
    import numpy as np
    blocks = []
    skipped = []
    for item in ordered_texts(reply["document"]):
        text = item["text"].strip()
        if not text:
            continue
        provenance = item.get("prov", [])
        if len(provenance) != 1:
            skipped.append({"text": text, "reason": "requires exactly one provenance box"})
            continue
        x, y, width, height = normalize_box(provenance[0], reply["width"], reply["height"])
        crop = image[math.floor(y):math.ceil(y + height), math.floor(x):math.ceil(x + width)]
        border = np.concatenate((crop[0], crop[-1], crop[:, 0], crop[:, -1]))
        background = np.median(border, axis=0)
        mask = np.linalg.norm(crop.astype(float) - background, axis=2) > 65
        active = np.sum(mask, axis=1) >= max(3, width * .008)
        spans = []
        for index, ink in enumerate(active):
            if ink and (not spans or index > spans[-1][1] + 2):
                spans.append([index, index])
            elif ink:
                spans[-1][1] = index
        spans = [s for s in spans if s[1] - s[0] + 1 >= 3]
        line_height = statistics.median(s[1] - s[0] + 1 for s in spans) if spans else height
        label = item.get("label", "text")
        kind = {"section_header": "heading", "title": "heading", "list_item": "list",
                "code": "code", "page_header": "metadata", "page_footer": "metadata"}.get(label, "paragraph")
        foreground = np.median(crop[mask], axis=0) if mask.any() else np.array([0, 0, 0])
        hexcolor = lambda bgr: "#" + "".join(f"{int(round(v)):02x}" for v in reversed(bgr))
        blocks.append(dict(x=x, y=y, width=width, height=height, lineHeight=line_height,
                           lineCount=max(1, len(spans)), kind=kind, align="left", text=text,
                           color=hexcolor(foreground), background=hexcolor(background),
                           sourceLabel=label, marker=item.get("marker"),
                           geometrySource="docling-provenance", fontEstimate="image-ink-projection"))
    return blocks, skipped


def percentile(values, fraction):
    return sorted(values)[max(0, math.ceil(len(values) * fraction) - 1)]


def windows_memory(process):
    """Resident and peak resident bytes for this worker, not system-wide usage."""
    if os.name != "nt":
        return None
    import ctypes
    from ctypes import wintypes
    class Counters(ctypes.Structure):
        _fields_ = [("cb", wintypes.DWORD), ("PageFaultCount", wintypes.DWORD)] + [
            (name, ctypes.c_size_t) for name in ("PeakWorkingSetSize", "WorkingSetSize",
            "QuotaPeakPagedPoolUsage", "QuotaPagedPoolUsage", "QuotaPeakNonPagedPoolUsage",
            "QuotaNonPagedPoolUsage", "PagefileUsage", "PeakPagefileUsage")]
    counters = Counters()
    counters.cb = ctypes.sizeof(counters)
    query = ctypes.windll.psapi.GetProcessMemoryInfo
    query.argtypes = [wintypes.HANDLE, ctypes.POINTER(Counters), wintypes.DWORD]
    query.restype = wintypes.BOOL
    if not query(wintypes.HANDLE(int(process._handle)), ctypes.byref(counters), counters.cb):
        raise ctypes.WinError()
    return dict(resident_bytes=counters.WorkingSetSize, peak_resident_bytes=counters.PeakWorkingSetSize)


def assess(report):
    cases = {case["name"]: case for case in report["cases"]}
    textract = [case for case in report["cases"] if "textract" in case["name"]]
    grouped = lambda case: case["engines"]["Docling.rs"]["blocks"]
    gates = {
        "textract_title_and_four_list_items_all_variants": bool(textract) and all(
            len(grouped(case)) == 5 and sum(b["kind"] == "heading" for b in grouped(case)) == 1
            and sum(b["kind"] == "list" for b in grouped(case)) == 4 for case in textract),
        "japanese_card_heading_preserved": any(b["kind"] == "heading" for b in grouped(cases["ocr-japanese-card.png"])),
        "search_two_metadata_regions_preserved": sum(b["kind"] == "metadata" for b in grouped(cases["ocr-search-results.png"])) >= 2,
        "observed_warm_p95_within_2000ms": all(case["engines"]["Docling.rs"]["summary"]["warm_p95_ms"] <= 2000 for case in report["cases"]),
        "no_character_error_regression": all(case["engines"]["Docling.rs"]["summary"]["quality"]["character_errors"]
            <= case["engines"]["RapidOCR Mobile"]["summary"]["quality"]["character_errors"] for case in report["cases"]),
    }
    return dict(gates=gates, integration_approved=all(gates.values()),
                note="Chưa đo dịch/renderer. Metadata là hợp đồng của OverText; nhãn Docling không tương đương trực tiếp. P95 chỉ có năm mẫu warm mỗi ảnh.")


def render_report(report, output):
    rows = []
    sections = []
    for case in report["cases"]:
        image = "data:image/png;base64," + base64.b64encode((ROOT / case["path"]).read_bytes()).decode()
        panes = [f'<div><h3>Ảnh gốc</h3><img src="{image}"></div>']
        for engine, data in case["engines"].items():
            summary = data["summary"]
            rows.append(f'<tr><td>{html.escape(case["name"])}</td><td>{engine}</td>'
                        f'<td>{summary["first_ms"]:.0f}</td><td>{summary["warm_median_ms"]:.0f}</td>'
                        f'<td>{summary["warm_p95_ms"]:.0f}</td><td>{len(data["blocks"])}</td>'
                        f'<td>{summary["quality"]["character_errors"]}/{summary["quality"]["reference_characters"]}</td></tr>')
            boxes = []
            for number, block in enumerate(data["blocks"], 1):
                boxes.append(f'<rect x="{block["x"]}" y="{block["y"]}" width="{block["width"]}" height="{block["height"]}"/>'
                             f'<text x="{block["x"]}" y="{max(12, block["y"])}">{number}</text>')
            svg = f'<svg viewBox="0 0 {case["width"]} {case["height"]}"><image href="{image}" width="100%" height="100%"/><g>{"".join(boxes)}</g></svg>'
            texts = "".join(f'<li><b>{b["kind"]}</b> ({b["lineCount"]} dòng): {html.escape(b["text"])}</li>' for b in data["blocks"])
            panes.append(f'<div><h3>{engine}</h3>{svg}<ol>{texts}</ol></div>')
        sections.append(f'<section><h2>{html.escape(case["name"])}</h2><p>{html.escape(case["note"])}</p><div class="panes">{"".join(panes)}</div></section>')
    page = '<!doctype html><html lang="vi"><meta charset="utf-8"><title>Thử nghiệm Docling.rs</title>'
    page += '<style>body{font:15px system-ui;margin:32px;color:#202630}table{border-collapse:collapse}td,th{padding:8px;border:1px solid #ccc}.panes{display:grid;grid-template-columns:repeat(3,minmax(0,1fr));gap:20px}img,svg{width:100%;height:auto}svg rect{fill:none;stroke:#e84824;stroke-width:1.5}svg text{fill:#d31;font:bold 14px sans-serif}li{margin:8px 0}section{margin:40px 0}</style>'
    page += '<h1>Thử nghiệm Docling.rs trên Windows CPU</h1><p>So sánh OCR và gom vùng. Khung đỏ là vùng nguồn; số đánh dấu thứ tự đọc. Chưa đo bản dịch hay chất lượng renderer. Thời gian gồm IPC; lần đầu mỗi ảnh được tách khỏi các lần warm.</p>'
    if "assessment" in report:
        page += '<h2>Kết luận</h2><p>' + ('Đạt các cổng thử nghiệm.' if report['assessment']['integration_approved'] else 'Chưa đạt điều kiện tích hợp vào Settings. Giữ RapidOCR Mobile hiện tại; prototype dùng để nghiên cứu tiếp hợp đồng layout.') + '</p><ul>'
        labels = {"textract_title_and_four_list_items_all_variants": "Textract: giữ một tiêu đề và bốn mục ở mọi biến thể",
                  "japanese_card_heading_preserved": "Thẻ tiếng Nhật: giữ vùng tiêu đề",
                  "search_two_metadata_regions_preserved": "Tìm kiếm: giữ hai vùng thông tin website riêng",
                  "observed_warm_p95_within_2000ms": "P95 quan sát không vượt 2 giây ở mọi ảnh",
                  "no_character_error_regression": "Không tăng lỗi ký tự so với RapidOCR ở mọi ảnh"}
        page += ''.join(f'<li>{html.escape(labels.get(name, name))}: <b>{"ĐẠT" if passed else "CHƯA ĐẠT"}</b></li>' for name, passed in report['assessment']['gates'].items()) + '</ul>'
    if all("memory" in data for case in report["cases"] for data in case["engines"].values()):
        peaks = {engine: max(case["engines"][engine]["memory"]["peak_resident_bytes"] for case in report["cases"]) / 1048576
                 for engine in ("Docling.rs", "RapidOCR Mobile")}
        page += '<p>Peak RAM của từng worker trong lần đo: ' + ', '.join(f'{engine} {value:.0f} MiB' for engine, value in peaks.items()) + '. RAM Docling gồm cả adapter layout dùng cho baseline. Đây không phải RAM tổng của ứng dụng.</p>'
    page += '<p>Ảnh Nhật chạy bằng lựa chọn <code>ch</code> để khảo sát; API Docling.rs chưa chấp nhận <code>ja</code>. Số lỗi ký tự bỏ qua dấu câu và so theo thứ tự vùng. P95 với ít mẫu chỉ là giá trị quan sát, chưa đủ đại diện sản xuất.</p>'
    page += '<p>Đối chiếu JSON thô: thẻ Nhật có một <code>&lt;!-- formula-not-decoded --&gt;</code> không có tọa độ thay cho vùng tiêu đề; adapter ghi nhận trong <code>skipped</code>. Ảnh tìm kiếm: đoạn mô tả đầu chứa cả website thứ hai, nhưng bbox nguồn vẫn chỉ bao đoạn mô tả đầu. Vì vậy không thể đưa trực tiếp các block này vào renderer để ghi đè an toàn. Ước lượng dòng từ ảnh trong adapter hiện chỉ dùng chẩn đoán.</p>'
    page += '<table><tr><th>Ảnh</th><th>Engine</th><th>Lần đầu ms</th><th>Warm median ms</th><th>Warm P95 ms</th><th>Blocks</th><th>Lỗi ký tự</th></tr>' + ''.join(rows) + '</table>'
    page += ''.join(sections) + '<h2>Cấu hình đo</h2><pre>' + html.escape(json.dumps(report['environment'], ensure_ascii=False, indent=2)) + '</pre></html>'
    output.write_text(page, encoding="utf-8", newline="\n")


def main():
    import cv2
    import numpy as np
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repeats", type=int, default=6, help="First run plus at least five warm runs")
    parser.add_argument("--worker", type=Path, default=ROOT / "logs/docling-rs-worker/target/release/overtext-docling-worker.exe")
    parser.add_argument("--output", type=Path, default=ROOT / "logs/docling-rs-comparison.json")
    parser.add_argument("--from-report", type=Path, help="Regenerate the HTML/assessment from saved measurements without inference")
    args = parser.parse_args()
    args.output.parent.mkdir(parents=True, exist_ok=True)
    if args.from_report:
        report = json.loads(args.from_report.read_text(encoding="utf-8"))
        for case in report["cases"]:
            case["path"] = Path(case["path"]).relative_to(ROOT).as_posix() if Path(case["path"]).is_absolute() else case["path"]
        report["assessment"] = assess(report)
        args.output.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8", newline="\n")
        render_report(report, args.output.with_suffix(".html"))
        return
    if args.repeats < 6:
        parser.error("use at least six repeats")
    sys.stdout.reconfigure(encoding="utf-8")
    spec = importlib.util.spec_from_file_location("scoring", ROOT / "scripts/benchmark-ocr-resize.py")
    scoring = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(scoring)
    textract_truth = """What is Amazon Textract?
Extract text and structured data such as tables and forms from documents using artificial intelligence (AI)—no configuration or templates necessary.
Go beyond simple optical character recognition (OCR) by extracting relationships, structure, and text from documents.
Improve security and compliance through robust data privacy, encryption, security controls, and support compliance standards such as HIPAA, GDPR, and more.
Easily implement human reviews with Amazon Augmented AI (A2I) to manage nuanced or sensitive workflows and audit predictions."""
    truths = {"ocr-textract.png": textract_truth, **scoring.IMAGES}
    variants = {}
    original = cv2.imdecode(np.fromfile(ROOT / "src-tauri/assets/ocr-textract.png", dtype=np.uint8), cv2.IMREAD_COLOR)
    for name, image in (("textract-scale075.png", cv2.resize(original, None, fx=.75, fy=.75, interpolation=cv2.INTER_AREA)),
                        ("textract-scale150.png", cv2.resize(original, None, fx=1.5, fy=1.5, interpolation=cv2.INTER_CUBIC)),
                        ("textract-crop.png", original[15:-15, 20:-20])):
        path = ROOT / "logs" / name
        cv2.imencode(".png", image)[1].tofile(path)
        variants[name] = path
        truths[name] = textract_truth
    runtime = Path(os.environ["LOCALAPPDATA"]) / "OverText/rapidocr"
    env = dict(os.environ, OVERTEXT_RAPID_DIR=str(runtime),
               ORT_DYLIB_PATH=str(runtime / "Lib/site-packages/onnxruntime/capi/onnxruntime.dll"),
               DOCLING_RS_MODELS_DIR=str(ROOT / "logs/docling-rs-models"),
               DOCLING_RS_GRAPH_CACHE_DIR=str(ROOT / "logs/docling-rs-graph-cache"),
               DOCLING_RS_PDF_THREADS="4", DOCLING_RS_PDF_INTRA="4", DOCLING_RS_OCR_SESSIONS="1",
               DOCLING_RS_EP="cpu", PYTHONUTF8="1")
    docling = Worker([str(args.worker.resolve())], env, ROOT / "logs/docling-rs-worker.stderr.log")
    rapid = None
    report = {"environment": {"docling_version": "1.104.2", "docling_revision": "29de9d1",
               "rapidocr_version": "3.9.2", "onnxruntime_version": "1.30.0", "cpu_threads": 4,
               "repeats": args.repeats, "docling_models": "Heron INT8 + PP-OCRv6 det/rec small",
               "tableformer": False, "translation_tested": False,
               "layout_sha256": hashlib.sha256((ROOT / "src-tauri/src/layout.rs").read_bytes()).hexdigest(),
               "cpu": os.environ.get("PROCESSOR_IDENTIFIER"), "logical_cpus": os.cpu_count()}, "cases": []}
    report["environment"]["worker_bytes"] = args.worker.stat().st_size
    report["environment"]["onnx_dll_bytes"] = Path(env["ORT_DYLIB_PATH"]).stat().st_size
    report["environment"]["docling_model_bytes"] = sum(p.stat().st_size for p in (ROOT / "logs/docling-rs-models").iterdir() if p.suffix in (".onnx", ".txt"))
    report["environment"]["model_sha256"] = {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in (ROOT / "logs/docling-rs-models").iterdir() if p.suffix in (".onnx", ".txt")}
    try:
        report["environment"]["ready"] = docling.receive()
        for name, truth in truths.items():
            path = variants.get(name, ROOT / "src-tauri/assets" / name)
            image = cv2.imdecode(np.fromfile(path, dtype=np.uint8), cv2.IMREAD_COLOR)
            height, width = image.shape[:2]
            case = dict(name=name, path=path.relative_to(ROOT).as_posix(), width=width, height=height,
                        note="Nhật: ch chỉ dùng khảo sát, API ja chưa hỗ trợ." if "japanese" in name else "Tiếng Anh.", engines={})
            report["cases"].append(case)
            # Run sequentially: no competing inference on the same CPU.
            for engine in ("Docling.rs", "RapidOCR Mobile"):
                runs = []
                if engine == "RapidOCR Mobile" and rapid is None:
                    rapid = Worker([str(runtime / "python.exe"), str(ROOT / "src-tauri/assets/rapidocr-worker.py")], env,
                                   ROOT / "logs/docling-rs-rapid.stderr.log")
                for repeat in range(args.repeats):
                    if engine == "Docling.rs":
                        reply, elapsed = docling.request(dict(image=str(path), language="ch" if "japanese" in name else "en"))
                        blocks, skipped = adapt_document(reply, image)
                    else:
                        reply, elapsed = rapid.request(dict(path=str(path), model="mobile"))
                        grouped, layout_ms = docling.request(dict(command="rapid-layout", image=str(path), lines=reply["lines"]))
                        elapsed += layout_ms
                        blocks, skipped = grouped["blocks"], []
                    runs.append(dict(ms=elapsed, reply=reply))
                    print(f"{name} {engine} {repeat+1}/{args.repeats}: {elapsed:.0f}ms, {len(blocks)} blocks", flush=True)
                expected = scoring.normalized(truth)
                actual = scoring.normalized("\n".join(b["text"] for b in blocks if not b["text"].strip().isdigit()))
                warm = [r["ms"] for r in runs[1:]]
                case["engines"][engine] = dict(blocks=blocks, skipped=skipped, runs=runs,
                    memory=windows_memory(docling.process if engine == "Docling.rs" else rapid.process),
                    summary=dict(first_ms=runs[0]["ms"], warm_median_ms=statistics.median(warm), warm_p95_ms=percentile(warm,.95),
                                 quality=dict(character_errors=scoring.distance(expected, actual), reference_characters=len(expected))))
                args.output.write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8", newline="\n")
        report["assessment"] = assess(report)
        args.output.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8", newline="\n")
        render_report(report, args.output.with_suffix(".html"))
    finally:
        docling.close()
        if rapid:
            rapid.close()
    print(args.output.with_suffix(".html"), flush=True)


if __name__ == "__main__":
    main()
