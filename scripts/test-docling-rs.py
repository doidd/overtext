"""Geometry and reading-order contract tests for the Docling.rs prototype."""
import importlib.util
import os
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location("benchmark", Path(__file__).with_name("benchmark-docling-rs.py"))
benchmark = importlib.util.module_from_spec(spec)
spec.loader.exec_module(benchmark)


class GeometryContract(unittest.TestCase):
    def test_top_left_and_bottom_left_resolve_to_same_pixels(self):
        top = {"page_no": 1, "bbox": dict(l=10, t=20, r=60, b=40, coord_origin="TOPLEFT")}
        bottom = {"page_no": 1, "bbox": dict(l=10, t=80, r=60, b=60, coord_origin="BOTTOMLEFT")}
        self.assertEqual(benchmark.normalize_box(top, 200, 100), (10, 20, 50, 20))
        self.assertEqual(benchmark.normalize_box(bottom, 200, 100), (10, 20, 50, 20))

    def test_invalid_boxes_are_rejected_instead_of_silently_moved(self):
        for box in (dict(l=60, t=20, r=10, b=40), dict(l=0, t=20, r=210, b=40),
                    dict(l=0, t=float("nan"), r=50, b=40)):
            with self.subTest(box=box), self.assertRaises(ValueError):
                benchmark.normalize_box({"page_no": 1, "bbox": dict(box, coord_origin="TOPLEFT")}, 200, 100)
        with self.assertRaises(ValueError):
            benchmark.normalize_box({"page_no": 2, "bbox": {}}, 200, 100)

    def test_body_tree_controls_reading_order(self):
        document = {"body": {"children": [{"$ref": "#/groups/0"}, {"$ref": "#/texts/0"}]},
                    "groups": [{"children": [{"$ref": "#/texts/1"}]}],
                    "texts": [{"text": "second"}, {"text": "first"}]}
        self.assertEqual([item["text"] for item in benchmark.ordered_texts(document)], ["first", "second"])

    def test_cyclic_references_fail(self):
        document = {"body": {"children": [{"$ref": "#/groups/0"}]},
                    "groups": [{"children": [{"$ref": "#/groups/0"}]}]}
        with self.assertRaises(ValueError):
            benchmark.ordered_texts(document)

    def test_grouped_paragraph_remains_one_block_and_missing_geometry_is_reported(self):
        import numpy as np
        image = np.full((100, 200, 3), 255, dtype=np.uint8)
        image[20:28, 10:100] = 0
        image[40:48, 10:100] = 0
        item = {"text": "One logical paragraph with two source rows", "label": "list_item", "marker": "✓",
                "prov": [{"page_no": 1, "bbox": dict(l=5, t=15, r=110, b=55, coord_origin="TOPLEFT")}]}
        missing = {"text": "No geometry", "label": "text"}
        reply = {"width": 200, "height": 100, "document": {
            "body": {"children": [{"$ref": "#/texts/0"}, {"$ref": "#/texts/1"}]}, "texts": [item, missing]}}
        blocks, skipped = benchmark.adapt_document(reply, image)
        self.assertEqual(len(blocks), 1)
        self.assertEqual(blocks[0]["text"], item["text"])
        self.assertEqual(blocks[0]["lineCount"], 2)
        self.assertEqual(blocks[0]["lineHeight"], 8)
        self.assertEqual(blocks[0]["marker"], "✓")
        self.assertEqual(blocks[0]["kind"], "list")
        self.assertEqual(skipped[0]["text"], "No geometry")


@unittest.skipUnless(os.environ.get("OVERTEXT_DOCLING_INTEGRATION") == "1", "Requires built worker and downloaded models")
class WorkerIntegration(unittest.TestCase):
    def test_error_reply_does_not_break_next_request(self):
        root = benchmark.ROOT
        runtime = Path(os.environ["LOCALAPPDATA"]) / "OverText/rapidocr"
        env = dict(os.environ, ORT_DYLIB_PATH=str(runtime / "Lib/site-packages/onnxruntime/capi/onnxruntime.dll"),
                   DOCLING_RS_MODELS_DIR=str(root / "logs/docling-rs-models"),
                   DOCLING_RS_GRAPH_CACHE_DIR=str(root / "logs/docling-rs-graph-cache"),
                   DOCLING_RS_PDF_THREADS="4", DOCLING_RS_EP="cpu")
        worker = benchmark.Worker([str(root / "logs/docling-rs-worker/target/release/overtext-docling-worker.exe")],
                                  env, root / "logs/docling-rs-integration.stderr.log")
        try:
            self.assertFalse(worker.receive()["warmed"])
            image = str(root / "src-tauri/assets/ocr-textract.png")
            with self.assertRaisesRegex(RuntimeError, "unsupported language"):
                worker.request(dict(image=image, language="ja"))
            reply, _ = worker.request(dict(image=image, language="en"))
            texts = benchmark.ordered_texts(reply["document"])
            self.assertEqual(sum(item["label"] == "list_item" for item in texts), 4)
            self.assertTrue(any("What is Amazon Textract" in item["text"] for item in texts))
        finally:
            worker.close()


if __name__ == "__main__":
    unittest.main()
