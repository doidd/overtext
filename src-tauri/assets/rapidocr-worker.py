"""Local RapidOCR ONNX worker; model downloads occur only in --install mode."""
import argparse
import json
import math
import os
from pathlib import Path
import sys
import time
import traceback


def normalized_lines(result, width, height):
    lines = []
    for text, polygon, score in zip(result["rec_texts"], result["rec_polys"], result["rec_scores"], strict=True):
        if not text.strip() or float(score) < .35:
            continue
        points = [(float(p[0]), float(p[1])) for p in polygon]
        if not points or not all(math.isfinite(v) for p in points for v in p):
            continue
        left = max(0.0, min(width, min(p[0] for p in points)))
        top = max(0.0, min(height, min(p[1] for p in points)))
        right = max(0.0, min(width, max(p[0] for p in points)))
        bottom = max(0.0, min(height, max(p[1] for p in points)))
        if right > left and bottom > top:
            lines.append(dict(text=text, x=left / width, y=top / height,
                              width=(right - left) / width, height=(bottom - top) / height))
    return lines


def model_files(model):
    if model not in ("mobile", "server"):
        raise ValueError("Unsupported RapidOCR model")
    return ["ch_PP-OCRv5_det_mobile.onnx", "ch_ppocr_mobile_v2.0_cls_mobile.onnx",
            f"ch_PP-OCRv5_rec_{model}.onnx"]


def offline_paths(directory, model):
    names = model_files(model)
    if not (directory / f"ready-{model}-v1").is_file():
        raise ValueError(f"RapidOCR {model} is not installed; install it in Settings")
    paths = {}
    for section, name in zip(("Det", "Cls", "Rec"), names, strict=True):
        path = directory / "models" / name
        if not path.is_file() or not path.stat().st_size:
            raise ValueError(f"Missing installed model {name}; reinstall it in Settings")
        paths[f"{section}.model_path"] = str(path)
    return paths


def create_engine(directory, model, download=False):
    model_files(model)
    paths = {} if download else offline_paths(directory, model)
    from rapidocr import EngineType, LangDet, LangRec, ModelType, OCRVersion, RapidOCR
    params = {
        "Global.model_root_dir": str(directory / "models"), "Global.use_cls": False,
        "Global.text_score": .35, "Global.max_side_len": 1600,
        "EngineConfig.onnxruntime.intra_op_num_threads": 4,
        "EngineConfig.onnxruntime.inter_op_num_threads": 1,
        "EngineConfig.onnxruntime.use_cuda": False, "EngineConfig.onnxruntime.use_dml": False,
        "Det.engine_type": EngineType.ONNXRUNTIME, "Det.lang_type": LangDet.CH,
        "Det.ocr_version": OCRVersion.PPOCRV5, "Det.model_type": ModelType.MOBILE,
        "Det.limit_side_len": 1600, "Det.limit_type": "max",
        "Det.box_thresh": .6, "Det.unclip_ratio": 1.5, "Det.use_dilation": False,
        "Rec.engine_type": EngineType.ONNXRUNTIME, "Rec.lang_type": LangRec.CH,
        "Rec.ocr_version": OCRVersion.PPOCRV5,
        "Rec.model_type": ModelType.MOBILE if model == "mobile" else ModelType.SERVER,
    }
    params.update(paths)
    return RapidOCR(params=params)


def predict(engine, path):
    import cv2
    import numpy as np
    image = cv2.imdecode(np.fromfile(path, dtype=np.uint8), cv2.IMREAD_COLOR)
    if image is None:
        raise ValueError("Cannot decode captured image")
    height, width = image.shape[:2]
    input_size = [width, height]
    scale = max(1.0, min(3.0, 1200 / max(width, height)))
    if scale > 1:
        image = cv2.resize(image, None, fx=scale, fy=scale, interpolation=cv2.INTER_CUBIC)
    height, width = image.shape[:2]
    started = time.perf_counter()
    result = engine(image, use_cls=False)
    elapsed = (time.perf_counter() - started) * 1000
    lines = [] if result.boxes is None else normalized_lines(
        dict(rec_texts=result.txts, rec_polys=result.boxes, rec_scores=result.scores), width, height)
    return lines, dict(input_size=input_size, ocr_size=[width, height], predict_ms=elapsed,
                       stage_ms=[None if v is None else float(v) * 1000 for v in (result.elapse_list or [])])


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--install", action="store_true")
    parser.add_argument("--model", choices=("mobile", "server"), default="mobile")
    parser.add_argument("--warmup")
    args = parser.parse_args()
    directory = Path(os.environ["OVERTEXT_RAPID_DIR"])
    if args.install:
        directory.mkdir(parents=True, exist_ok=True)
        engine = create_engine(directory, args.model, download=True)
        if not args.warmup or not predict(engine, args.warmup)[0]:
            raise ValueError("RapidOCR installation verification produced no text")
        ready = directory / f"ready-{args.model}-v1"
        temporary = ready.with_suffix(".tmp")
        temporary.write_text(f"RapidOCR 3.9.2 / ONNX Runtime 1.30.0 / PP-OCRv5 {args.model}\n", encoding="utf-8")
        temporary.replace(ready)
        print(f"RapidOCR {args.model} ready; models downloaded and verified", flush=True)
        return
    protocol = os.fdopen(os.dup(sys.stdout.fileno()), "w", encoding="utf-8", buffering=1)
    os.dup2(sys.stderr.fileno(), sys.stdout.fileno())
    sys.stdout = sys.stderr
    sys.stdin.reconfigure(encoding="utf-8")
    engine = None
    current_model = None
    for request in sys.stdin:
        started = time.perf_counter()
        timings = {}
        try:
            data = json.loads(request)
            model = data["model"]
            model_files(model)
            timings.update(request_id=data.get("request_id"), model=model, model_reused=current_model == model)
            init = time.perf_counter()
            if current_model != model:
                engine = None
                current_model = None
                engine = create_engine(directory, model)
                current_model = model
            timings["model_init_ms"] = (time.perf_counter() - init) * 1000
            lines, stage = predict(engine, data["path"])
            timings.update(stage, retained_lines=len(lines), device="cpu", cpu_threads=4)
            reply = {"lines": lines}
        except Exception as error:
            traceback.print_exc(file=sys.stderr)
            reply = {"error": str(error)}
            timings["failed"] = True
        timings["total_ms"] = (time.perf_counter() - started) * 1000
        print("rapidocr timing: " + json.dumps(timings, allow_nan=False), file=sys.stderr, flush=True)
        protocol.write(json.dumps(reply, ensure_ascii=False, allow_nan=False) + "\n")
        protocol.flush()


if __name__ == "__main__":
    main()
