"""Prepare/score a fixed OCR corpus. No engine is downloaded or selected implicitly.

prepare: downloads only selected public images and pinned annotation files.
score: scores raw desktop-test results and exports OmniDocBench-compatible inputs.
The custom region metrics are intentionally stricter than Markdown quick_match.
"""
import argparse
import base64
from collections import defaultdict
import hashlib
import html
import importlib.util
import json
import math
import os
from pathlib import Path
import statistics
import unicodedata
import urllib.request
import urllib.parse

ROOT = Path(__file__).resolve().parents[1]
CACHE = ROOT / "logs/ocr-benchmark"
DATASETS = {
    "omnidocbench": dict(repo="opendatalab/OmniDocBench", revision="aa1ee96d106dbe53d0ae59474d75c6e6d9b53fec",
                        annotation="OmniDocBench.json", sha256="a45cd84b04ad8b793e775089640e6b681209abea33ead54c1828ddca35fae496", language="en-US"),
    "jasyn": dict(repo="stockmark/OmniDocBench-JASyn", revision="73ecb24624682575bd5ebf138026e711589ae5c6",
                  annotation="OmniDocBench_JASyn.json", sha256="d6ef2b628626d73ae430a548005d872ff83f7477aeeb1441fd3fda8caccd99a8", language="ja"),
}
TEXT_CATEGORIES = {"text_block", "title", "header", "footer", "caption", "list_item", "page_number"}
TEXTRACT = ["What is Amazon Textract?",
    "Extract text and structured data such as tables and forms from documents using artificial intelligence (AI)—no configuration or templates necessary.",
    "Go beyond simple optical character recognition (OCR) by extracting relationships, structure, and text from documents.",
    "Improve security and compliance through robust data privacy, encryption, security controls, and support compliance standards such as HIPAA, GDPR, and more.",
    "Easily implement human reviews with Amazon Augmented AI (A2I) to manage nuanced or sensitive workflows and audit predictions."]


def write_json(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n", encoding="utf-8", newline="\n")


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def download(url, target, checksum=None):
    target.parent.mkdir(parents=True, exist_ok=True)
    if not target.exists():
        temporary = target.with_suffix(target.suffix + ".download")
        with urllib.request.urlopen(urllib.parse.quote(url,safe=":/?=&%"), timeout=120) as source, temporary.open("wb") as dest:
            while chunk := source.read(1024 * 1024):
                dest.write(chunk)
        temporary.replace(target)
    if checksum and digest(target) != checksum:
        raise ValueError(f"Checksum mismatch: {target}")


def polygon_box(poly):
    if len(poly) < 6 or len(poly) % 2 or not all(math.isfinite(float(v)) for v in poly):
        raise ValueError("Invalid dataset polygon")
    xs, ys = poly[::2], poly[1::2]
    return dict(x=min(xs), y=min(ys), width=max(xs)-min(xs), height=max(ys)-min(ys))


def region(text, kind, order, box):
    return dict(text=text, kind=kind, order=order, x=box[0], y=box[1], width=box[2], height=box[3])


def local_cases():
    spec = importlib.util.spec_from_file_location("scoring", ROOT / "scripts/benchmark-ocr-resize.py")
    scoring = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(scoring)
    japanese = scoring.JAPANESE.splitlines()
    cases = []
    definitions = {
        "ocr-textract.png": ("en-US", TEXTRACT, ["heading"]+["list"]*4,
                             [(37,24,391,40),(37,94,480,82),(37,193,466,52),(37,259,477,79),(37,355,480,78)]),
        "ocr-japanese-card.png": ("ja", japanese, ["heading"]+["list"]*4,
                                  [(93,19,166,27),(25,54,299,22),(25,79,212,22),(25,104,270,22),(25,128,185,23)]),
        "ocr-japanese.png": ("ja", japanese, ["heading"]+["paragraph"]*4,
                             [(91,14,163,27),(23,49,283,23),(23,75,217,23),(23,99,275,23),(23,124,205,23)]),
        "ocr-japanese-list.png": ("ja", japanese, ["heading"]+["list"]*4,
                                  [(116,31,177,32),(34,74,342,23),(34,101,245,23),(34,128,299,23),(34,155,240,23)]),
    }
    for name, (language, texts, kinds, boxes) in definitions.items():
        cases.append(dict(id="local-"+Path(name).stem, image="src-tauri/assets/"+name, language=language,
                          dataset="local", annotation_source="manually transcribed text and image region boxes; not OCR-derived",
                          regions=[region(text,kind,i,box) for i,(text,kind,box) in enumerate(zip(texts,kinds,boxes))]))
    search = scoring.SEARCH.splitlines()
    texts = ["\n".join(search[:2]),search[2],"\n".join(search[3:5]),"\n".join(search[5:7]),search[7],"\n".join(search[8:])]
    boxes = [(53,8,374,41),(19,57,451,27),(19,85,601,43),(19,153,338,42),(19,201,587,25),(19,228,611,43)]
    kinds = ["metadata","heading","paragraph","metadata","heading","paragraph"]
    cases.append(dict(id="local-search",image="src-tauri/assets/ocr-search-results.png",language="en-US",dataset="local",
                      annotation_source="manually transcribed text and image region boxes; not OCR-derived",
                      regions=[region(t,k,i,b) for i,(t,k,b) in enumerate(zip(texts,kinds,boxes))]))
    return cases


def prepare(limit):
    import cv2
    import numpy as np
    cases = local_cases()
    source = next(c for c in cases if c["id"] == "local-ocr-textract")
    original = cv2.imdecode(np.fromfile(ROOT/source["image"],dtype=np.uint8),cv2.IMREAD_COLOR)
    for name, scale, offset in (("scale075",.75,(0,0)),("scale150",1.5,(0,0)),("crop",1.,(20,15))):
        image = original[15:-15,20:-20] if name == "crop" else cv2.resize(original,None,fx=scale,fy=scale,interpolation=cv2.INTER_AREA if scale<1 else cv2.INTER_CUBIC)
        path = CACHE/"images"/f"local-textract-{name}.png"
        path.parent.mkdir(parents=True,exist_ok=True)
        cv2.imencode(".png",image)[1].tofile(path)
        transformed = [dict(r,x=(r["x"]-offset[0])*scale,y=(r["y"]-offset[1])*scale,
                            width=r["width"]*scale,height=r["height"]*scale) for r in source["regions"]]
        cases.append(dict(source,id=f"local-textract-{name}",image=path.relative_to(ROOT).as_posix(),regions=transformed,
                          variant=dict(scale=scale,crop_offset=list(offset))))
    manifests = {}
    for dataset, config in DATASETS.items():
        base = f"https://huggingface.co/datasets/{config['repo']}/resolve/{config['revision']}/"
        annotation = CACHE/"datasets"/config["annotation"]
        download(base+config["annotation"],annotation,config["sha256"])
        pages = json.loads(annotation.read_text(encoding="utf-8"))
        candidates = []
        for page in pages:
            info = page["page_info"]
            attributes = info["page_attribute"]
            if dataset == "omnidocbench" and (attributes.get("language")!="english" or attributes.get("data_source")!="PPT2PDF"):
                continue
            if dataset == "jasyn" and attributes.get("layout") not in ("slide","report","pamphlet"):
                continue
            if any(d.get("latex") or d.get("category_type")=="table" or "\\" in d.get("text","") or "$" in d.get("text","") for d in page["layout_dets"]):
                continue
            regions = []
            for d in page["layout_dets"]:
                if d.get("ignore") or d["category_type"] not in TEXT_CATEGORIES or not d.get("text","").strip():
                    continue
                kind = {"title":"heading","header":"metadata","footer":"metadata","page_number":"metadata","list_item":"list"}.get(d["category_type"],"paragraph")
                regions.append(dict(text=d["text"],kind=kind,order=d["order"] if d.get("order") is not None else 100000+len(regions),
                                    reading_order_known=d.get("order") is not None,
                                    category=d["category_type"],annotation_id=d["anno_id"],**polygon_box(d["poly"])))
            if not 100 <= sum(len(r["text"]) for r in regions) <= 1200:
                continue
            candidates.append((page,regions))
        # Fixed selection independent of recognition scores. Slides first, then name.
        candidates.sort(key=lambda pair:(pair[0]["page_info"]["page_attribute"].get("layout")!="slide",pair[0]["page_info"]["image_path"]))
        selected = candidates[:limit]
        if len(selected)!=limit:
            raise ValueError(f"Only {len(selected)} eligible samples in {dataset}")
        manifests[dataset]=dict(config,selected_images=[p["page_info"]["image_path"] for p,_ in selected])
        for page,regions in selected:
            info = page["page_info"]
            name = info["image_path"]
            if Path(name).name != name:
                raise ValueError("Unexpected dataset image path")
            original_path = CACHE/"datasets"/dataset/name
            download(base+"images/"+name,original_path)
            image = cv2.imdecode(np.fromfile(original_path,dtype=np.uint8),cv2.IMREAD_COLOR)
            if image is None or (image.shape[1],image.shape[0]) != (info["width"],info["height"]):
                raise ValueError(f"Image/annotation size mismatch: {name}")
            png = CACHE/"images"/(dataset+"-"+Path(name).stem+".png")
            png.parent.mkdir(parents=True,exist_ok=True)
            cv2.imencode(".png",image)[1].tofile(png)
            cases.append(dict(id=dataset+"-"+Path(name).stem,image=png.relative_to(ROOT).as_posix(),language=config["language"],
                              dataset=dataset,regions=sorted(regions,key=lambda r:r["order"]),page_attribute=info["page_attribute"],
                              original_image_sha256=digest(original_path),original_page=page))
    for case in cases:
        path=ROOT/case["image"]
        image=cv2.imdecode(np.fromfile(path,dtype=np.uint8),cv2.IMREAD_COLOR)
        case.update(width=image.shape[1],height=image.shape[0],sha256=digest(path))
        for r in case["regions"]:
            if r["x"]<0 or r["y"]<0 or r["width"]<=0 or r["height"]<=0 or r["x"]+r["width"]>case["width"] or r["y"]+r["height"]>case["height"]:
                raise ValueError(f"Ground-truth box outside image: {case['id']}")
    manifest=dict(version=1,datasets=manifests,cases=cases,selection="horizontal slides/report/pamphlet; 100–1200 annotated text chars; no tables/LaTeX; fixed filename order")
    write_json(CACHE/"manifest.json",manifest)
    print(f"Prepared {len(cases)} cases: {CACHE/'manifest.json'}",flush=True)


def normalized(text):
    return "".join(c for c in unicodedata.normalize("NFKC",text).lower() if c.isalnum())


def edit_distance(a,b):
    previous=list(range(len(b)+1))
    for i,left in enumerate(a,1):
        current=[i]
        for j,right in enumerate(b,1):
            current.append(min(current[-1]+1,previous[j]+1,previous[j-1]+(left!=right)))
        previous=current
    return previous[-1]


def intersection(a,b):
    return max(0,min(a["x"]+a["width"],b["x"]+b["width"])-max(a["x"],b["x"])) * max(0,min(a["y"]+a["height"],b["y"]+b["height"])-max(a["y"],b["y"]))


def best_region(box,regions):
    area=box["width"]*box["height"]
    if not math.isfinite(area) or area<=0:
        return None
    matches=[intersection(box,r)/area for r in regions]
    if not matches or max(matches)<.5:
        return None
    return max(range(len(matches)),key=matches.__getitem__)


def horizontal_reading_order(lines):
    rows=[]
    for line in sorted(lines,key=lambda line:line["y"]+line["height"]/2):
        center=line["y"]+line["height"]/2
        if rows and abs(center-statistics.median(v["y"]+v["height"]/2 for v in rows[-1])) <= .4*max(line["height"],statistics.median(v["height"] for v in rows[-1])):
            rows[-1].append(line)
        else:
            rows.append([line])
    return [line for row in rows for line in sorted(row,key=lambda line:line["x"])]


def measure(case,run):
    regions=case["regions"]
    lines=[]
    for line in run["lines"]:
        lines.append(dict(line,x=line["x"]*case["width"],y=line["y"]*case["height"],
                          width=line["width"]*case["width"],height=line["height"]*case["height"]))
    assignments=[best_region(line,regions) for line in lines]
    per_region=[]
    touched=defaultdict(set)
    merges=defaultdict(set)
    uncontained=0
    for line,reference in zip(lines,assignments):
        block=best_region(line,run["blocks"])
        if block is None:
            uncontained+=1
        elif reference is not None:
            touched[reference].add(block)
            merges[block].add(reference)
    for index,r in enumerate(regions):
        members=[line for line,assigned in zip(lines,assignments) if assigned==index]
        # Horizontal rows only; vertical-text evaluation is a separate future subset.
        members=horizontal_reading_order(members)
        actual=" ".join(line["text"] for line in members)
        expected=normalized(r["text"])
        per_region.append(dict(index=index,expected=r["text"],actual=actual,errors=edit_distance(expected,normalized(actual)),characters=len(expected),detected=bool(members)))
    sequence=[]
    for block in run["blocks"]:
        reference=best_region(block,regions)
        if reference is not None and regions[reference].get("reading_order_known",True) and reference not in sequence:
            sequence.append(reference)
    inversions=sum(regions[sequence[i]]["order"]>regions[sequence[j]]["order"] for i in range(len(sequence)) for j in range(i+1,len(sequence)))
    pairs=len(sequence)*(len(sequence)-1)//2
    errors=sum(r["errors"] for r in per_region)
    characters=sum(r["characters"] for r in per_region)
    return dict(character_errors=errors,reference_characters=characters,cer=errors/max(1,characters),
                missing_regions=sum(not r["detected"] for r in per_region),split_regions=sum(len(v)>1 for v in touched.values()),
                merged_blocks=sum(len(v)>1 for v in merges.values()),reading_order_inversions=inversions,reading_order_pairs=pairs,
                unassigned_ocr_lines=assignments.count(None),uncontained_ocr_lines=uncontained,regions=per_region)


def percentile(values,p):
    return sorted(values)[max(0,math.ceil(len(values)*p)-1)]


def score(raw_path,output):
    raw=json.loads(raw_path.read_text(encoding="utf-8"))
    cases={c["id"]:c for c in raw["corpus"]["cases"]}
    for case in cases.values():
        if digest(ROOT/case["image"])!=case["sha256"]:
            raise ValueError(f"Corpus image changed after preparation: {case['id']}")
    summaries=[]
    exports=output.parent/"official-inputs"
    ground_truth=defaultdict(list)
    for case in cases.values():
        if "original_page" in case:
            ground_truth[case["dataset"]].append(case["original_page"])
    for dataset,pages in ground_truth.items():
        write_json(exports/dataset/"ground-truth.json",pages)
    for result in raw["results"]:
        case=cases[result["case"]]
        valid=[r for r in result["runs"] if "error" not in r]
        if len(valid)!=raw["repeats"]:
            summaries.append(dict(case=case["id"],engine=result["engine"],error=[r.get("error") for r in result["runs"] if "error" in r]))
            continue
        scores=[measure(case,r) for r in valid]
        warm=[r["total_ms"] for r in valid[1:]]
        summary=dict(case=case["id"],dataset=case["dataset"],language=case["language"],engine=result["engine"],
                     first_request_ms=valid[0]["total_ms"],warm_median_ms=statistics.median(warm),warm_p95_ms=percentile(warm,.95),
                     score=scores[-1],scores=scores,scores_stable=all(q==scores[0] for q in scores),blocks=valid[-1]["blocks"],lines=valid[-1]["lines"])
        summaries.append(summary)
        if "original_page" in case:
            directory=exports/case["dataset"]/result["engine"]
            directory.mkdir(parents=True,exist_ok=True)
            stem=Path(case["original_page"]["page_info"]["image_path"]).stem
            markdown=[]
            for b in valid[-1]["blocks"]:
                prefix={"heading":"# ","list":"- "}.get(b["kind"],"")
                markdown.append(prefix+b["text"])
            (directory/(stem+".md")).write_text("\n\n".join(markdown)+"\n",encoding="utf-8",newline="\n")
    for dataset in ground_truth:
        for engine in sorted({r["engine"] for r in raw["results"]}):
            config=f"""end2end_eval:
  metrics:
    text_block:
      metric: [Edit_dist]
    reading_order:
      metric: [Edit_dist]
  dataset:
    dataset_name: end2end_dataset
    ground_truth:
      data_path: {json.dumps(str((exports/dataset/'ground-truth.json').resolve()).replace(chr(92),'/'))}
    prediction:
      data_path: {json.dumps(str((exports/dataset/engine).resolve()).replace(chr(92),'/'))}
    match_method: quick_match
"""
            (exports/dataset/(engine+".yaml")).write_text(config,encoding="utf-8",newline="\n")
    report=dict(version=1,repeats=raw["repeats"],windows_languages=raw["windows_languages"],datasets=raw["corpus"]["datasets"],
                layout_sha256=digest(ROOT/"src-tauri/src/layout.rs"),summaries=summaries,
                corpus_sha256=hashlib.sha256(json.dumps(raw["corpus"],sort_keys=True,ensure_ascii=False).encode()).hexdigest(),
                environment=dict(cpu=os.environ.get("PROCESSOR_IDENTIFIER"),logical_cpus=os.cpu_count(),
                                 timing="desktop exploratory; background load not controlled; engine calls sequential"),
                scope="Custom region metrics on a fixed small subset, not official OmniDocBench scores. No translation-provider evaluation.")
    write_json(output,report)
    write_html(report,cases,output.with_suffix(".html"))
    print(output.with_suffix(".html"),flush=True)


def write_html(report,cases,output):
    import cv2
    import numpy as np
    rows=[]
    sections=[]
    for s in report["summaries"]:
        if "error" in s:
            rows.append(f'<tr><td>{html.escape(s["case"])}</td><td>{s["engine"]}</td><td colspan="7">ERROR: {html.escape(str(s["error"]))}</td></tr>')
            continue
        q=s["score"]
        rows.append(f'<tr><td>{html.escape(s["case"])}</td><td>{s["engine"]}</td><td>{s["warm_median_ms"]:.0f}</td><td>{s["warm_p95_ms"]:.0f}</td><td>{q["cer"]:.1%}</td><td>{q["missing_regions"]}</td><td>{q["split_regions"]}</td><td>{q["merged_blocks"]}</td><td>{q["reading_order_inversions"]}/{q["reading_order_pairs"]}</td></tr>')
    for case in cases.values():
        image=cv2.imdecode(np.fromfile(ROOT/case["image"],dtype=np.uint8),cv2.IMREAD_COLOR)
        scale=min(1,1000/image.shape[1])
        if scale<1:image=cv2.resize(image,None,fx=scale,fy=scale,interpolation=cv2.INTER_AREA)
        preview=cv2.imencode(".jpg",image,[cv2.IMWRITE_JPEG_QUALITY,88])[1].tobytes()
        source="data:image/jpeg;base64,"+base64.b64encode(preview).decode()
        def overlay(boxes):
            return f'<svg viewBox="0 0 {case["width"]} {case["height"]}"><image href="{source}" width="100%" height="100%"/><g>'+''.join(f'<rect x="{b["x"]}" y="{b["y"]}" width="{b["width"]}" height="{b["height"]}"/><text x="{b["x"]}" y="{max(14,b["y"])}">{i+1}</text>' for i,b in enumerate(boxes))+'</g></svg>'
        panes=['<div><h3>Nhãn tham chiếu</h3>'+overlay(case["regions"])+'</div>']
        for s in report["summaries"]:
            if s["case"]!=case["id"]:continue
            content=html.escape(str(s["error"])) if "error" in s else overlay(s["blocks"])+"<ol>"+''.join(f'<li><b>{b["kind"]}</b> {html.escape(b["text"])}</li>' for b in s["blocks"])+"</ol>"
            panes.append(f'<div><h3>{s["engine"]}</h3>{content}</div>')
        sections.append(f'<section><h2>{html.escape(case["id"])}</h2><p>{case["dataset"]} · {case["language"]} · {case["width"]}×{case["height"]}</p><div class="panes">'+''.join(panes)+'</div></section>')
    page='<!doctype html><html lang="vi"><meta charset="utf-8"><title>Benchmark Windows OCR và RapidOCR</title><style>body{font:14px system-ui;margin:24px;color:#202630}table{border-collapse:collapse;width:100%}td,th{padding:6px;border:1px solid #ccc}.panes{display:grid;grid-template-columns:repeat(4,minmax(0,1fr));gap:16px}svg{width:100%;height:auto}rect{fill:none;stroke:#e84824;stroke-width:2}svg text{fill:#d31;font:bold 18px sans-serif}li{margin:8px 0}section{margin:40px 0}</style>'
    page+='<h1>Windows OCR · RapidOCR Mobile · RapidOCR Server</h1><p>Tập nhỏ cố định: ảnh lỗi thực tế và mẫu tiếng Anh/Nhật từ OmniDocBench/JASyn. Nhãn có trước kết quả engine. Đây là chỉ số tự đo theo vùng, không phải điểm OmniDocBench chính thức. Chỉ chạy OCR/layout; chưa đo dịch thật. Khung đỏ là vùng chữ nguồn; ảnh minh họa là preview JPEG tối đa 1000px ngang, các phép đo dùng PNG đầy đủ.</p><p>Mỗi ảnh bỏ lượt đầu; P95 với năm lượt warm chỉ là số quan sát. CER bỏ qua dấu câu/khoảng trắng và chấm riêng từng vùng nguồn. Thiếu vùng nghĩa là không có dòng OCR khớp vùng; không đồng nghĩa toàn bộ chữ đúng. Tách/gộp dựa vào dòng OCR khớp vùng tham chiếu và block dự đoán, chỉ phản ánh các dòng đã nhận diện. Thứ tự đọc chỉ chấm các vùng đã ghép được; cần xem cùng số vùng thiếu. Text trong hình/bảng ngoài nhãn text không được chấm CER. Không suy luận engine tốt nhất chỉ từ một chỉ số. Thời gian là số thăm dò trên desktop, chưa kiểm soát tải nền.</p>'
    page+='<table><tr><th>Ảnh</th><th>Engine</th><th>Warm median ms</th><th>P95 ms</th><th>CER</th><th>Thiếu vùng</th><th>Tách vùng</th><th>Gộp block</th><th>Đảo thứ tự/cặp</th></tr>'+''.join(rows)+'</table>'+''.join(sections)+'</html>'
    official=[]
    for dataset in ("omnidocbench","jasyn"):
        for engine in ("windows","rapid-mobile","rapid-server"):
            metric=output.parent/"official-results"/dataset/engine/(engine+"_quick_match_metric_result.json")
            provenance=metric.parent/"input-sha256.json"
            if metric.exists() and provenance.exists():
                hashes=json.loads(provenance.read_text(encoding="utf-8"))
                inputs=output.parent/"official-inputs"/dataset
                expected={"ground-truth.json",*(engine+"/"+p.name for p in (inputs/engine).glob("*.md"))}
                if set(hashes)!=expected or any(digest(inputs/p)!=value for p,value in hashes.items()):
                    continue
                scores=json.loads(metric.read_text(encoding="utf-8"))
                official.append(dict(dataset=dataset,engine=engine,
                    text_edit=scores["text_block"]["all"]["Edit_dist"]["ALL_page_avg"],
                    reading_order_edit=scores["reading_order"]["all"]["Edit_dist"]["ALL_page_avg"]))
    overview='<p><a href="renderer.html">Xem kiểm tra ghi đè, clipping và DPI của renderer</a></p>'
    write_json(output.parent/"official-summary.json",dict(matcher="quick_match",scores=official))
    if official:
        overview+='<h2>Evaluator OmniDocBench chính thức — tập con 3 trang Anh + 3 trang Nhật</h2><p>Edit_dist càng thấp càng tốt; không tương đương CER theo vùng bên dưới. Không đo bảng/công thức.</p><table><tr><th>Tập</th><th>Engine</th><th>Text Edit_dist</th><th>Reading order Edit_dist</th></tr>'
        overview+=''.join(f'<tr><td>{r["dataset"]}</td><td>{r["engine"]}</td><td>{r["text_edit"]:.4f}</td><td>{r["reading_order_edit"]:.4f}</td></tr>' for r in official)+'</table>'
    overview+='<p>OmniDocBench: dữ liệu chỉ dùng nghiên cứu, không đưa vào bộ cài. JASyn: CC BY 4.0, nhãn theo block. Nguồn: <a href="https://huggingface.co/datasets/opendatalab/OmniDocBench">OmniDocBench</a>, <a href="https://huggingface.co/datasets/stockmark/OmniDocBench-JASyn">JASyn</a>. Mẫu nhỏ, trong đó hai trang Nhật cùng tài liệu; chưa đủ kết luận cho mọi ngôn ngữ.</p>'
    page=page.replace('<h1>',overview+'<h1>',1)
    output.write_text(page,encoding="utf-8",newline="\n")


if __name__=="__main__":
    parser=argparse.ArgumentParser(description=__doc__)
    commands=parser.add_subparsers(dest="command",required=True)
    p=commands.add_parser("prepare");p.add_argument("--public-per-language",type=int,default=3)
    s=commands.add_parser("score");s.add_argument("--raw",type=Path,default=CACHE/"raw.json");s.add_argument("--output",type=Path,default=CACHE/"summary.json")
    args=parser.parse_args()
    if args.command=="prepare":
        if args.public_per_language<1:parser.error("public-per-language must be positive")
        prepare(args.public_per_language)
    else:score(args.raw,args.output)
