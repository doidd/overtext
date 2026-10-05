"""RapidOCR/ONNX CPU comparison on the existing OCR fixtures and quality gate.

Install isolated dependencies using the Windows Paddle runtime's Python:
python -m pip install --target logs/rapidocr-deps rapidocr==3.9.2 onnxruntime==1.30.0
Run this script with the same Python, not a Python of another ABI/version.
"""
import argparse
import hashlib
import importlib.metadata
import importlib.util
import json
from pathlib import Path
import statistics
import sys
import time

ROOT = Path(__file__).resolve().parents[1]


def load_source(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def main():
    sys.stdout.reconfigure(encoding="utf-8")
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--deps", type=Path, default=ROOT / "logs/rapidocr-deps")
    parser.add_argument("--repeats", type=int, default=3)
    args = parser.parse_args()
    if args.repeats < 3:
        parser.error("at least three repeats are required")
    sys.path.insert(0, str(args.deps.resolve()))
    import cv2
    import numpy as np
    from rapidocr import EngineType, LangDet, LangRec, ModelType, OCRVersion, RapidOCR
    scoring = load_source("ocr_scoring", ROOT / "scripts/benchmark-ocr-resize.py")
    worker = load_source("ocr_geometry", ROOT / "src-tauri/assets/paddleocr-worker.py")
    logs = ROOT / "logs"
    models = logs / "rapidocr-models"
    models.mkdir(parents=True, exist_ok=True)
    baseline = json.loads((logs / "ocr-resize-comparison.json").read_text(encoding="utf-8"))["cases"]["1200"]["summary"]
    report = {"repeats": args.repeats, "upscale_target": 1200, "cpu_threads": 4,
              "backend": "onnxruntime CPUExecutionProvider", "orientation_classifier": False,
              "versions": {name: importlib.metadata.version(name) for name in ("rapidocr", "onnxruntime", "numpy", "opencv-python")},
              "cases": {}}
    for model in ("server", "mobile"):
        params = {
            "Global.model_root_dir": str(models), "Global.use_cls": False,
            "Global.text_score": .35, "Global.max_side_len": 1600,
            "EngineConfig.onnxruntime.intra_op_num_threads": 4,
            "EngineConfig.onnxruntime.inter_op_num_threads": 1,
            "EngineConfig.onnxruntime.use_cuda": False,
            "EngineConfig.onnxruntime.use_dml": False,
            "Det.engine_type": EngineType.ONNXRUNTIME, "Det.lang_type": LangDet.CH,
            "Det.ocr_version": OCRVersion.PPOCRV5, "Det.model_type": ModelType.MOBILE,
            "Det.limit_side_len": 1600, "Det.limit_type": "max",
            "Det.box_thresh": .6, "Det.unclip_ratio": 1.5, "Det.use_dilation": False,
            "Rec.engine_type": EngineType.ONNXRUNTIME, "Rec.lang_type": LangRec.CH,
            "Rec.ocr_version": OCRVersion.PPOCRV5,
            "Rec.model_type": ModelType.SERVER if model == "server" else ModelType.MOBILE,
        }
        print(f"Starting RapidOCR PP-OCRv5 {model}; models may download on the first call", flush=True)
        started = time.perf_counter()
        engine = RapidOCR(params=params)
        case = report["cases"][model] = {"init_ms": (time.perf_counter() - started) * 1000,
            "params": {k: getattr(v, "value", v) for k, v in params.items()}, "summary": {}, "runs": {}}
        for name, truth in scoring.IMAGES.items():
            runs = []
            for repeat in range(args.repeats):
                started = time.perf_counter()
                image = cv2.imdecode(np.fromfile(ROOT / "src-tauri/assets" / name, dtype=np.uint8), cv2.IMREAD_COLOR)
                if image is None:
                    raise RuntimeError(f"Failed to decode {name}")
                h, w = image.shape[:2]
                scale = worker.upscale_factor(w, h, 1200)
                if scale > 1:
                    image = cv2.resize(image, None, fx=scale, fy=scale, interpolation=cv2.INTER_CUBIC)
                h, w = image.shape[:2]
                output = engine(image, use_cls=False)
                if output.boxes is None or not output.txts:
                    raise RuntimeError(f"Empty OCR output: {name}")
                lines = worker.normalized_lines(dict(rec_texts=output.txts, rec_polys=output.boxes,
                                                      rec_scores=output.scores), w, h)
                elapsed = (time.perf_counter() - started) * 1000
                runs.append({"repeat": repeat + 1, "total_ms": elapsed, "ocr_size": [w, h], "lines": lines,
                             "stage_ms": [None if v is None else float(v) * 1000 for v in output.elapse_list],
                             "quality": scoring.quality(lines, truth)})
            scores = [run["quality"] for run in runs]
            warm = statistics.median(run["total_ms"] for run in runs[1:])
            good = (max(s["character_errors"] for s in scores) <= max(s["character_errors"] for s in baseline[name]["scores"])
                    and all(not s["missing_keywords"] for s in scores))
            summary = {"warm_ms": warm, "initial_ms": runs[0]["total_ms"], "scores": scores,
                       "line_counts": [len(run["lines"]) for run in runs], "quality_pass": good,
                       "speedup": baseline[name]["warm_ms"] / warm}
            case["summary"][name], case["runs"][name] = summary, runs
            print(f"RapidOCR {model} {name}: warm={warm:.0f}ms speedup={summary['speedup']:.2f}x "
                  f"errors={[s['character_errors'] for s in scores]} quality={'PASS' if good else 'FAIL'}", flush=True)
            report["model_files"] = [{"name": p.name, "sha256": hashlib.sha256(p.read_bytes()).hexdigest()}
                                     for p in sorted(models.rglob("*.onnx"))]
            (logs / "ocr-rapidocr-comparison.json").write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
        print(f"RapidOCR {model}: all-image quality gate={'PASS' if all(s['quality_pass'] for s in case['summary'].values()) else 'FAIL'}", flush=True)
        del engine


if __name__ == "__main__":
    main()
