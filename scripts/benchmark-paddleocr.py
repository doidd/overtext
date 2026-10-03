"""Compare CPU backends on identical images using the installed Windows runtime."""
import json
import os
from pathlib import Path
import statistics
import subprocess
import sys

sys.stdout.reconfigure(encoding="utf-8")
root = Path(__file__).resolve().parents[1]
runtime = Path(os.environ["LOCALAPPDATA"]) / "OverText/paddleocr"
images = [root / "src-tauri/assets" / name for name in
          ("ocr-japanese-card.png", "ocr-japanese.png")]
requests = [dict(path=str(image), language="", request_id=i * 3 + repeat + 1)
            for i, image in enumerate(images) for repeat in range(3)]
logs = root / "logs"
logs.mkdir(exist_ok=True)
report = {}
for enabled in (False, True):
    env = os.environ.copy()
    env.update(PYTHONUTF8="1", PADDLE_PDX_CACHE_HOME=str(runtime / "models"),
               OVERTEXT_OCR_MKLDNN="1" if enabled else "0")
    result = subprocess.run([str(runtime / "python.exe"), "-u",
                             str(root / "src-tauri/assets/paddleocr-worker.py")],
                            input="".join(json.dumps(request) + "\n" for request in requests),
                            capture_output=True, text=True, encoding="utf-8", env=env, timeout=180)
    label = "enabled" if enabled else "disabled"
    (logs / f"mkldnn-{label}.log").write_text(result.stderr, encoding="utf-8")
    replies = [json.loads(line) for line in result.stdout.splitlines() if line.strip()]
    timings = [json.loads(line.removeprefix("paddleocr timing: "))
               for line in result.stderr.splitlines() if line.startswith("paddleocr timing: {")]
    report[label] = dict(returncode=result.returncode, replies=replies, timings=timings)
    (logs / "mkldnn-comparison.json").write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
    if result.returncode or len(replies) != len(requests) or any("error" in reply for reply in replies):
        print(label, "FAILED", json.dumps(replies, ensure_ascii=False), flush=True)
        continue
    for i, image in enumerate(images):
        warm = timings[i * 3 + 1:i * 3 + 3]
        print(label, image.name, "first_request_ms=", timings[i * 3]["total_ms"],
              "warm_median_ms=", statistics.median(item["total_ms"] for item in warm),
              "recognition_ms=", statistics.median(item["recognition_ms"] for item in warm),
              flush=True)
if all(len(report[label]["replies"]) == len(requests) and
       all("lines" in reply for reply in report[label]["replies"]) for label in report):
    for i, request in enumerate(requests):
        before, after = (report[label]["replies"][i]["lines"] for label in ("disabled", "enabled"))
        texts_equal = [line["text"] for line in before] == [line["text"] for line in after]
        geometry_delta = max((abs(a[key] - b[key]) for a, b in zip(before, after)
                              for key in ("x", "y", "width", "height")), default=0)
        print("COMPARE", request["request_id"], "lines=", (len(before), len(after)),
              "texts_equal=", texts_equal, "max_box_delta=", geometry_delta, flush=True)
