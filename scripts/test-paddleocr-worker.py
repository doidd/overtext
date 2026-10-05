"""Geometry and language regressions; needs only Python's standard library."""
import importlib.util
from pathlib import Path
import unittest
from unittest.mock import patch

worker_path = Path(__file__).resolve().parents[1] / "src-tauri/assets/paddleocr-worker.py"
spec = importlib.util.spec_from_file_location("paddle_worker", worker_path)
worker = importlib.util.module_from_spec(spec)
spec.loader.exec_module(worker)
benchmark_spec = importlib.util.spec_from_file_location("ocr_benchmark", Path(__file__).with_name("benchmark-ocr-resize.py"))
benchmark = importlib.util.module_from_spec(benchmark_spec)
benchmark_spec.loader.exec_module(benchmark)


class WorkerTests(unittest.TestCase):
    def test_quality_uses_geometry_order_and_ignores_typography_and_badges(self):
        lines = [dict(text="整理", x=0, y=.5), dict(text="4", x=.9, y=0),
                 dict(text="デ ー タ 加 工", x=0, y=.1)]
        score = benchmark.quality(lines, "データ加工・整理")
        self.assertEqual(score["character_errors"], 0)
        self.assertNotIn("データ加工", score["missing_keywords"])

    def test_quality_detects_changed_japanese_characters(self):
        score = benchmark.quality([dict(text="デー夕加工", x=0, y=0)], "データ加工")
        self.assertEqual(score["character_errors"], 1)
        self.assertIn("データ加工", score["missing_keywords"])

    def test_resize_policy_never_downscales_and_caps_upscaling(self):
        self.assertEqual(worker.upscale_factor(400, 200, 1200), 3)
        self.assertEqual(worker.upscale_factor(400, 200, 800), 2)
        self.assertEqual(worker.upscale_factor(400, 200, 0), 1)
        self.assertEqual(worker.upscale_factor(1203, 627, 1200), 1)
        self.assertEqual(worker.upscale_factor(100, 50, 1200), 3)
        for target in (-1, 1601):
            with self.assertRaises(ValueError):
                worker.upscale_factor(400, 200, target)

    def test_model_timer_excludes_consumer_time_and_preserves_results(self):
        clock = [0.0]
        def predictor():
            for result in ("first", "second"):
                clock[0] += 2
                yield result
            clock[0] += 1
        timed = worker.TimedPredictor(predictor)
        with patch.object(worker.time, "perf_counter", side_effect=lambda: clock[0]):
            iterator = timed()
            self.assertEqual(next(iterator), "first")
            clock[0] += 10  # Layout/consumer work must not count as model time.
            self.assertEqual(next(iterator), "second")
            with self.assertRaises(StopIteration):
                next(iterator)
        self.assertEqual(timed.elapsed, 5.0)

    def test_japanese_and_chinese_share_multilingual_model(self):
        with patch.dict(worker.os.environ, {"OVERTEXT_OCR_MULTILINGUAL_MODEL": "PP-OCRv5_server_rec"}):
            for language in ("", "ja-JP", "JA-jp", "zh-Hans-CN", "zh-Hant-TW"):
                self.assertEqual(worker.model_for_language(language), "PP-OCRv5_server_rec")
        self.assertEqual(worker.model_for_language("en-US"), "en_PP-OCRv5_mobile_rec")
        self.assertEqual(worker.model_for_language("ko-KR"), "korean_PP-OCRv5_mobile_rec")
        with self.assertRaises(ValueError):
            worker.model_for_language("xx-ZZ")

    def test_mobile_override_only_applies_to_multilingual_languages(self):
        with patch.dict(worker.os.environ, {"OVERTEXT_OCR_MULTILINGUAL_MODEL": "PP-OCRv5_mobile_rec"}):
            self.assertEqual(worker.model_for_language("ja-JP"), "PP-OCRv5_mobile_rec")
            self.assertEqual(worker.model_for_language(""), "PP-OCRv5_mobile_rec")
            self.assertEqual(worker.model_for_language("en-US"), "en_PP-OCRv5_mobile_rec")
        with patch.dict(worker.os.environ, {"OVERTEXT_OCR_MULTILINGUAL_MODEL": "invalid"}):
            with self.assertRaises(ValueError):
                worker.model_for_language("")

    def test_rotated_polygon_clipped_and_normalized_for_overlay(self):
        result = dict(rec_texts=["データ加工"], rec_scores=[0.9],
                      rec_polys=[[[-10, 10], [90, -2], [110, 40], [0, 50]]])
        lines = worker.normalized_lines(result, 100, 100)
        self.assertEqual(lines, [dict(text="データ加工", x=0.0, y=0.0, width=1.0, height=0.5)])

    def test_upscaling_keeps_identical_normalized_geometry(self):
        result = dict(rec_texts=["整理"], rec_scores=[0.9],
                      rec_polys=[[[10, 20], [80, 20], [80, 40], [10, 40]]])
        original = worker.normalized_lines(result, 100, 100)
        result["rec_polys"] = [[[x * 3, y * 3] for x, y in result["rec_polys"][0]]]
        self.assertEqual(worker.normalized_lines(result, 300, 300), original)

    def test_empty_low_confidence_and_invalid_boxes_are_skipped(self):
        result = dict(rec_texts=[" ", "uncertain", "outside", "invalid"],
                      rec_scores=[0.9, 0.1, 0.9, 0.9],
                      rec_polys=[[[0, 0], [10, 10]], [[0, 0], [10, 10]],
                                 [[110, 110], [120, 120]], [[float("nan"), 0]]])
        self.assertEqual(worker.normalized_lines(result, 100, 100), [])

    def test_mismatched_results_do_not_silently_drop_text(self):
        with self.assertRaises(ValueError):
            worker.normalized_lines(dict(rec_texts=["text"], rec_scores=[], rec_polys=[]), 100, 100)


if __name__ == "__main__":
    unittest.main()
