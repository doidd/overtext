"""Controlled real OCR comparison: fixed model, threads, MKL-DNN, images and truth.

Runs one isolated worker per size policy. First run of each image is excluded
from warm medians. Full replies/timings are saved for content and box review.
The production worker keeps its 1200 default until a quality gate justifies it.
"""
import argparse
import json
import os
from pathlib import Path
import statistics
import subprocess
import sys
import unicodedata


ROOT = Path(__file__).resolve().parents[1]
JAPANESE = """データ加工・整理
弊社側で保持していない項目の付加（例：重要度）
分類／名寄せ、集計軸の整備
現データと過去データの統合（共通指摘ID）
分析要件に合わせた加工"""
SEARCH = """drupalreleases.com
https://www.drupalreleases.com › project › simple-crawler
Simple Crawler - Drupal Module | Drupal Releases
Simple Crawler helps you scrape or even crawl webpages and websites for context, research or
migrations. It just a wrapper around ...
whatisdrupal.com
https://whatisdrupal.com › how-to-optimize-your...
How to Optimize Your Drupal Website for AI Crawlers and Data ...
Optimizing a Drupal website for AI crawlers requires a clean and organized content management
foundation that adheres to ..."""
IMAGES = {"ocr-japanese-card.png": JAPANESE, "ocr-japanese.png": JAPANESE,
          "ocr-japanese-list.png": JAPANESE, "ocr-search-results.png": SEARCH}


def normalized(text):
    # Ignore typography/punctuation while retaining letters, numbers and scripts.
    return "".join(c for c in unicodedata.normalize("NFKC", text).lower() if c.isalnum())


def distance(a, b):
    previous = list(range(len(b) + 1))
    for i, left in enumerate(a, 1):
        current = [i]
        for j, right in enumerate(b, 1):
            current.append(min(current[-1] + 1, previous[j] + 1,
                               previous[j - 1] + (left != right)))
        previous = current
    return previous[-1]


def quality(lines, truth):
    # Engines may return rows in reverse order. Compare image reading order,
    # as layout does, rather than counting ordering differences as OCR errors.
    text = "\n".join(line["text"] for line in sorted(lines, key=lambda line: (line["y"], line["x"]))
                     if not line["text"].strip().isdigit())
    expected, actual = normalized(truth), normalized(text)
    keywords = ["データ加工", "重要度", "分類", "集計軸", "共通指摘ID", "分析要件"] if "データ" in truth else [
        "drupalreleases.com", "Simple Crawler", "Drupal Releases", "whatisdrupal.com",
        "Optimizing a Drupal website", "content management", "foundation"]
    missing = [word for word in keywords if normalized(word) not in actual]
    return dict(text=text, character_errors=distance(expected, actual),
                reference_characters=len(expected), missing_keywords=missing)


def main():
    sys.stdout.reconfigure(encoding="utf-8")
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repeats", type=int, default=3, help="At least 3: one initial and two warm runs")
    parser.add_argument("--model", choices=("server", "mobile"), default="server")
    parser.add_argument("--targets", type=int, nargs="+", default=[1200, 800, 0])
    parser.add_argument("--native-report", action="store_true", help="Score the ignored Rust native benchmark against the same truth")
    parser.add_argument("--rescore", action="store_true", help="Recompute quality from saved replies without running models")
    args = parser.parse_args()
    if args.repeats < 3:
        parser.error("at least three repeats are required")
    runtime = Path(os.environ["LOCALAPPDATA"]) / "OverText/paddleocr"
    logs = ROOT / "logs"
    logs.mkdir(exist_ok=True)
    if args.rescore:
        for name in ("ocr-resize-comparison.json", "ocr-mobile-comparison.json"):
            path = logs / name
            report = json.loads(path.read_text(encoding="utf-8"))
            for case in report["cases"].values():
                for i, (image, truth) in enumerate(IMAGES.items()):
                    begin = i * report["repeats"]
                    case["summary"][image]["scores"] = [quality(r["lines"], truth)
                        for r in case["replies"][begin:begin + report["repeats"]]]
            path.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
        print("Rescored saved replies in image reading order")
        return
    if args.native_report:
        native = json.loads((logs / "ocr-native-comparison.json").read_text(encoding="utf-8"))
        baseline = json.loads((logs / "ocr-resize-comparison.json").read_text(encoding="utf-8"))["cases"]["1200"]["summary"]
        summary = {}
        for case in native:
            name = case["image"]
            scores = [quality(run["lines"], IMAGES[name]) for run in case["runs"]]
            warm = statistics.median(run["ms"] for run in case["runs"][1:])
            good = (max(s["character_errors"] for s in scores) <= max(s["character_errors"] for s in baseline[name]["scores"])
                    and all(not s["missing_keywords"] for s in scores))
            summary[name] = dict(warm_ms=warm, scores=scores, quality_pass=good)
            print(f"native {name}: warm={warm:.0f}ms speedup={baseline[name]['warm_ms']/warm:.2f}x "
                  f"errors={[s['character_errors'] for s in scores]} quality={'PASS' if good else 'FAIL'}", flush=True)
        (logs / "ocr-native-quality.json").write_text(json.dumps(summary, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
        return
    report = {"repeats": args.repeats, "model": args.model, "cases": {}}
    report_name = "ocr-resize-comparison.json" if args.model == "server" else "ocr-mobile-comparison.json"
    requests = [dict(path=str(ROOT / "src-tauri/assets" / name), language="", request_id=len(IMAGES) * repeat + i + 1)
                for i, name in enumerate(IMAGES) for repeat in range(args.repeats)]
    for target in args.targets:
        env = os.environ.copy()
        env.update(PYTHONUTF8="1", PADDLE_PDX_CACHE_HOME=str(runtime / "models"),
                   OVERTEXT_OCR_MKLDNN="1", OVERTEXT_OCR_UPSCALE_SIDE=str(target),
                   OVERTEXT_OCR_MULTILINGUAL_MODEL=f"PP-OCRv5_{args.model}_rec")
        result = subprocess.run([str(runtime / "python.exe"), "-u", str(ROOT / "src-tauri/assets/paddleocr-worker.py")],
                                input="".join(json.dumps(r) + "\n" for r in requests),
                                capture_output=True, text=True, encoding="utf-8", env=env, timeout=600)
        (logs / f"ocr-{args.model}-{target}.log").write_text(result.stderr, encoding="utf-8")
        replies = [json.loads(line) for line in result.stdout.splitlines() if line.strip()]
        timings = [json.loads(line.removeprefix("paddleocr timing: ")) for line in result.stderr.splitlines()
                   if line.startswith("paddleocr timing: {")]
        if result.returncode or len(replies) != len(requests) or len(timings) != len(requests) or any("error" in r for r in replies):
            raise RuntimeError(f"Failed target={target}; see logs/ocr-{args.model}-{target}.log")
        case = report["cases"][str(target)] = dict(replies=replies, timings=timings, summary={})
        for i, (name, truth) in enumerate(IMAGES.items()):
            begin = i * args.repeats
            warm = timings[begin + 1:begin + args.repeats]
            scores = [quality(r["lines"], truth) for r in replies[begin:begin + args.repeats]]
            summary = case["summary"][name] = dict(
                initial_ms=timings[begin]["total_ms"],
                warm_ms=statistics.median(t["total_ms"] for t in warm),
                recognition_ms=statistics.median(t["recognition_ms"] for t in warm),
                line_counts=[len(r["lines"]) for r in replies[begin:begin + args.repeats]],
                scores=scores)
            print(f"target={target} {name}: warm={summary['warm_ms']:.0f}ms "
                  f"errors={[s['character_errors'] for s in scores]} "
                  f"missing={[s['missing_keywords'] for s in scores]}", flush=True)
        (logs / report_name).write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    reference = report if args.model == "server" else json.loads((logs / "ocr-resize-comparison.json").read_text(encoding="utf-8"))
    for target in args.targets:
        if args.model == "server" and target == 1200:
            continue
        passing = True
        for name in IMAGES:
            baseline = reference["cases"]["1200"]["summary"][name]
            candidate = report["cases"][str(target)]["summary"][name]
            good = (max(s["character_errors"] for s in candidate["scores"]) <= max(s["character_errors"] for s in baseline["scores"])
                    and all(not s["missing_keywords"] for s in candidate["scores"]))
            passing &= good
            print(f"target={target} {name}: speedup={baseline['warm_ms']/candidate['warm_ms']:.2f}x quality={'PASS' if good else 'FAIL'}")
        print(f"target={target}: all-image quality gate={'PASS' if passing else 'FAIL'}")


if __name__ == "__main__":
    main()
