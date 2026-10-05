"""RapidOCR geometry/offline-install contract; standard library only."""
import importlib.util
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
from types import SimpleNamespace

root = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("rapid_worker", root / "src-tauri/assets/rapidocr-worker.py")
worker = importlib.util.module_from_spec(spec)
spec.loader.exec_module(worker)


class RapidTests(unittest.TestCase):
    def test_blank_image_returns_empty_lines(self):
        image = SimpleNamespace(shape=(1200, 1200, 3))
        cv2 = SimpleNamespace(imdecode=lambda *_: image, IMREAD_COLOR=1)
        numpy = SimpleNamespace(fromfile=lambda *_, **__: b"image", uint8=object())
        result = SimpleNamespace(boxes=None, elapse_list=None)
        with patch.dict("sys.modules", {"cv2": cv2, "numpy": numpy}):
            lines, timings = worker.predict(lambda *_, **__: result, "blank.png")
        self.assertEqual(lines, [])
        self.assertEqual(timings["stage_ms"], [])

    def test_failed_install_verification_never_marks_model_ready(self):
        with tempfile.TemporaryDirectory() as temp, \
                patch.dict("os.environ", {"OVERTEXT_RAPID_DIR": temp}), \
                patch("sys.argv", ["worker", "--install", "--warmup", "warmup.png"]), \
                patch.object(worker, "create_engine", return_value=object()), \
                patch.object(worker, "predict", return_value=([], {})):
            with self.assertRaises(ValueError):
                worker.main()
            self.assertFalse((Path(temp) / "ready-mobile-v1").exists())

    def test_models_are_explicit_and_invalid_names_rejected(self):
        self.assertIn("ch_PP-OCRv5_rec_mobile.onnx", worker.model_files("mobile"))
        self.assertIn("ch_PP-OCRv5_rec_server.onnx", worker.model_files("server"))
        with self.assertRaises(ValueError):
            worker.model_files("../other")

    def test_ready_marker_cannot_hide_missing_or_empty_model_files(self):
        with tempfile.TemporaryDirectory() as temp:
            directory = Path(temp)
            with self.assertRaises(ValueError):
                worker.offline_paths(directory, "mobile")
            (directory / "ready-mobile-v1").write_text("ready")
            (directory / "models").mkdir()
            with self.assertRaises(ValueError):
                worker.offline_paths(directory, "mobile")
            for name in worker.model_files("mobile"):
                (directory / "models" / name).write_bytes(b"model")
            paths = worker.offline_paths(directory, "mobile")
            self.assertEqual(len(paths), 3)
            self.assertTrue(all(Path(path).is_file() for path in paths.values()))
            with self.assertRaises(ValueError):
                worker.offline_paths(directory, "server")
            (directory / "models" / worker.model_files("mobile")[-1]).write_bytes(b"")
            with self.assertRaises(ValueError):
                worker.offline_paths(directory, "mobile")

    def test_normalized_geometry_matches_paddle_contract(self):
        result = dict(rec_texts=["データ加工", "low"], rec_scores=[.9, .1],
                      rec_polys=[[[-10, 10], [90, -2], [110, 40], [0, 50]], [[0, 0], [10, 10]]])
        self.assertEqual(worker.normalized_lines(result, 100, 100), [dict(text="データ加工", x=0, y=0, width=1, height=.5)])
        with self.assertRaises(ValueError):
            worker.normalized_lines(dict(rec_texts=["missing"], rec_scores=[], rec_polys=[]), 100, 100)


if __name__ == "__main__":
    unittest.main()
