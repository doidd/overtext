"""Checks that benchmark metrics detect regressions without rewarding missing text."""
import importlib.util
from pathlib import Path
import unittest

spec=importlib.util.spec_from_file_location("corpus",Path(__file__).with_name("ocr-corpus.py"))
corpus=importlib.util.module_from_spec(spec)
spec.loader.exec_module(corpus)


def box(x,y,w,h,**extra):
    return dict(x=x,y=y,width=w,height=h,**extra)


class CorpusMetrics(unittest.TestCase):
    def setUp(self):
        self.case=dict(width=100,height=100,regions=[
            box(0,10,90,10,text="Alpha beta",kind="paragraph",order=0),
            box(0,40,90,10,text="Gamma",kind="paragraph",order=1)])
        self.lines=[box(0,.1,.9,.1,text="Alpha beta"),box(0,.4,.9,.1,text="Gamma")]
        self.blocks=[box(0,10,90,10),box(0,40,90,10)]

    def test_correct_regions_have_zero_errors(self):
        q=corpus.measure(self.case,dict(lines=self.lines,blocks=self.blocks))
        for name in ["character_errors","missing_regions","split_regions","merged_blocks","reading_order_inversions","uncontained_ocr_lines"]:
            self.assertEqual(q[name],0,name)

    def test_missing_line_penalizes_text_and_coverage(self):
        q=corpus.measure(self.case,dict(lines=self.lines[:1],blocks=self.blocks[:1]))
        self.assertEqual(q["missing_regions"],1)
        self.assertEqual(q["character_errors"],5)
        self.assertEqual(q["reading_order_pairs"],0)

    def test_overmerge_detected_even_when_all_text_is_correct(self):
        q=corpus.measure(self.case,dict(lines=self.lines,blocks=[box(0,10,90,40)]))
        self.assertEqual(q["character_errors"],0)
        self.assertEqual(q["merged_blocks"],1)

    def test_fragmentation_detected_and_row_fragments_read_left_to_right(self):
        lines=[box(.51,.09,.39,.11,text="beta"),box(0,.1,.49,.1,text="Alpha"),self.lines[1]]
        blocks=[box(0,10,49,10),box(51,9,39,11),self.blocks[1]]
        q=corpus.measure(self.case,dict(lines=lines,blocks=blocks))
        self.assertEqual(q["character_errors"],0)
        self.assertEqual(q["split_regions"],1)

    def test_reading_order_is_separate_from_ocr_accuracy(self):
        q=corpus.measure(self.case,dict(lines=self.lines,blocks=list(reversed(self.blocks))))
        self.assertEqual(q["character_errors"],0)
        self.assertEqual(q["reading_order_inversions"],1)
        self.assertEqual(q["reading_order_pairs"],1)

    def test_unknown_reference_order_is_not_invented(self):
        self.case["regions"][0]["reading_order_known"]=False
        q=corpus.measure(self.case,dict(lines=self.lines,blocks=list(reversed(self.blocks))))
        self.assertEqual(q["reading_order_pairs"],0)
        self.assertEqual(q["reference_characters"],14)

    def test_polygon_and_local_annotations_are_independent_of_predictions(self):
        self.assertEqual(corpus.polygon_box([10,20,40,20,40,50,10,50]),box(10,20,30,30))
        self.assertEqual(len(corpus.local_cases()),5)
        self.assertEqual(corpus.normalized("ＡＩ — 日本語"),"ai日本語")


if __name__=="__main__":unittest.main()
