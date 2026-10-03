"""Private JSON-lines worker. Screenshots remain local; only models are downloaded."""
import json
import math
import os
import sys
import time
import traceback

ENABLE_MKLDNN = os.environ.get("OVERTEXT_OCR_MKLDNN", "1") != "0"


class TimedPredictor:
    """Time lazy model execution without including time spent by its consumer."""
    def __init__(self, predictor):
        self.predictor = predictor
        self.elapsed = 0.0

    def __getattr__(self, name):
        return getattr(self.predictor, name)

    def __call__(self, *args, **kwargs):
        started = time.perf_counter()
        iterator = iter(self.predictor(*args, **kwargs))
        self.elapsed += time.perf_counter() - started
        while True:
            started = time.perf_counter()
            try:
                item = next(iterator)
            except StopIteration:
                return
            finally:
                self.elapsed += time.perf_counter() - started
            yield item


def instrument_models(engine):
    # PaddleOCR/PaddleX versions are pinned by the installer. Profiling is
    # optional if a future version changes the internal pipeline structure.
    pipeline = getattr(engine.paddlex_pipeline, "_pipeline", engine.paddlex_pipeline)
    if not all(hasattr(pipeline, name) for name in ("text_det_model", "text_rec_model")):
        print("paddleocr timing: model stage profiling unavailable", file=sys.stderr)
        return None, None
    detection = TimedPredictor(pipeline.text_det_model)
    recognition = TimedPredictor(pipeline.text_rec_model)
    pipeline.text_det_model = detection
    pipeline.text_rec_model = recognition
    return detection, recognition


def model_for_language(language):
    tag = language.lower().split("-")[0]
    if tag in ("", "ja", "zh"):
        return "PP-OCRv5_server_rec"
    if tag == "en":
        return "en_PP-OCRv5_mobile_rec"
    family = {
        "ko": "korean", "fr": "latin", "de": "latin", "es": "latin",
        "pt": "latin", "it": "latin", "vi": "latin", "id": "latin",
        "ru": "eslav", "uk": "eslav", "be": "eslav", "ar": "arabic",
        "hi": "devanagari", "th": "th",
    }.get(tag)
    if family is None:
        raise ValueError(f"PaddleOCR fallback does not support source language {language}")
    return f"{family}_PP-OCRv5_mobile_rec"


def normalized_lines(result, width, height):
    lines = []
    for text, polygon, score in zip(result["rec_texts"], result["rec_polys"], result["rec_scores"], strict=True):
        if not text.strip() or float(score) < 0.35:
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


def main():
    # Native inference libraries can write directly to fd 1. Preserve a private
    # protocol descriptor before routing both native and Python logs to stderr.
    protocol = os.fdopen(os.dup(sys.stdout.fileno()), "w", encoding="utf-8", buffering=1)
    os.dup2(sys.stderr.fileno(), sys.stdout.fileno())
    sys.stdout = sys.stderr
    sys.stdin.reconfigure(encoding="utf-8")
    os.environ.setdefault("PADDLE_PDX_DISABLE_MODEL_SOURCE_CHECK", "True")
    engine = None
    current_model = None
    detection = recognition = None
    for request in sys.stdin:
        started = time.perf_counter()
        timings = {}
        try:
            data = json.loads(request)
            timings["request_id"] = data.get("request_id")
            model = model_for_language(data["language"])
            stage = time.perf_counter()
            from paddleocr import PaddleOCR
            import cv2
            import numpy as np
            timings["imports_ms"] = (time.perf_counter() - stage) * 1000
            timings["model"] = model
            timings["model_reused"] = current_model == model
            stage = time.perf_counter()
            if current_model != model:
                engine = None
                engine = PaddleOCR(
                    text_detection_model_name="PP-OCRv5_mobile_det",
                    text_recognition_model_name=model,
                    use_doc_orientation_classify=False, use_doc_unwarping=False,
                    use_textline_orientation=False, device="cpu",
                    enable_mkldnn=ENABLE_MKLDNN, cpu_threads=4,
                    text_det_limit_side_len=1600, text_det_limit_type="max",
                )
                current_model = model
                detection, recognition = instrument_models(engine)
            timings["model_init_ms"] = (time.perf_counter() - stage) * 1000
            if detection is not None:
                detection.elapsed = recognition.elapsed = 0.0
            stage = time.perf_counter()
            # imdecode handles Unicode Windows paths that cv2.imread can reject.
            image = cv2.imdecode(np.fromfile(data["path"], dtype=np.uint8), cv2.IMREAD_COLOR)
            if image is None:
                raise ValueError("Cannot decode captured image")
            height, width = image.shape[:2]
            timings["input_size"] = [width, height]
            timings["decode_ms"] = (time.perf_counter() - stage) * 1000
            stage = time.perf_counter()
            if max(width, height) < 1200:
                scale = min(3.0, 1200 / max(width, height))
                image = cv2.resize(image, None, fx=scale, fy=scale, interpolation=cv2.INTER_CUBIC)
            height, width = image.shape[:2]
            timings["ocr_size"] = [width, height]
            timings["resize_ms"] = (time.perf_counter() - stage) * 1000
            lines = []
            normalize_ms = 0.0
            detected = recognized = 0
            stage = time.perf_counter()
            for result in engine.predict(image):
                detected += len(result["dt_polys"])
                recognized += len(result["rec_texts"])
                normalize_started = time.perf_counter()
                lines.extend(normalized_lines(result, width, height))
                normalize_ms += (time.perf_counter() - normalize_started) * 1000
            timings["predict_ms"] = (time.perf_counter() - stage) * 1000
            timings["detection_ms"] = detection.elapsed * 1000 if detection else None
            timings["recognition_ms"] = recognition.elapsed * 1000 if recognition else None
            timings["normalize_ms"] = normalize_ms
            timings["detected_regions"] = detected
            timings["recognized_regions"] = recognized
            timings["retained_lines"] = len(lines)
            timings["device"] = "cpu"
            timings["cpu_threads"] = 4
            timings["mkldnn"] = ENABLE_MKLDNN
            reply = {"lines": lines}
        except Exception as exc:
            traceback.print_exc(file=sys.stderr)
            reply = {"error": str(exc)}
            timings["failed"] = True
        timings["total_ms"] = (time.perf_counter() - started) * 1000
        timings = {key: round(value, 2) if isinstance(value, float) else value
                   for key, value in timings.items()}
        print("paddleocr timing: " + json.dumps(timings, allow_nan=False), file=sys.stderr, flush=True)
        protocol.write(json.dumps(reply, ensure_ascii=False, allow_nan=False) + "\n")
        protocol.flush()


if __name__ == "__main__":
    main()
