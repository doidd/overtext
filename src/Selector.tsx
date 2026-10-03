import { useEffect, useRef, useState, type PointerEvent } from "react";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";

type Props = { monitor: number; imagePath: string; width: number; height: number };
type Point = { x: number; y: number };
type Rect = { x: number; y: number; width: number; height: number };

/** Selections smaller than this (CSS px) are treated as stray clicks. */
const MIN_SIZE = 4;

function toRect(a: Point, b: Point): Rect {
  return {
    x: Math.min(a.x, b.x),
    y: Math.min(a.y, b.y),
    width: Math.abs(a.x - b.x),
    height: Math.abs(a.y - b.y),
  };
}

export function Selector({ monitor, imagePath, width }: Props) {
  const [anchor, setAnchor] = useState<Point | null>(null);
  const [cursor, setCursor] = useState<Point | null>(null);
  const [pixelRatio, setPixelRatio] = useState(1);
  const submitted = useRef(false);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") invoke("cancel_capture");
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const rect = anchor && cursor ? toRect(anchor, cursor) : null;

  const point = (e: PointerEvent): Point => ({ x: e.clientX, y: e.clientY });

  const onPointerDown = (e: PointerEvent<HTMLDivElement>) => {
    if (e.button !== 0 || submitted.current) return;
    e.currentTarget.setPointerCapture(e.pointerId);
    setAnchor(point(e));
    setCursor(point(e));
  };

  const onPointerMove = (e: PointerEvent) => {
    if (anchor) setCursor(point(e));
  };

  const onPointerUp = () => {
    if (!rect) return;
    if (rect.width < MIN_SIZE || rect.height < MIN_SIZE) {
      setAnchor(null);
      setCursor(null);
      return;
    }
    submitted.current = true;
    invoke("finish_capture", { selection: { monitor, ...rect } }).catch((error) =>
      invoke("cancel_capture", { error: String(error) }),
    );
  };

  return (
    <div
      className="selector"
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onContextMenu={(e) => {
        e.preventDefault();
        invoke("cancel_capture");
      }}
    >
      <img
        className="frozen"
        src={convertFileSrc(imagePath)}
        draggable={false}
        onLoad={async (e) => {
          const img = e.currentTarget;
          setPixelRatio(img.naturalWidth / width);
          await img.decode().catch(() => {});
          invoke("window_ready");
        }}
        onError={() => invoke("cancel_capture", { error: `cannot load ${imagePath}` })}
      />
      {rect ? (
        <div className="selection" style={{ left: rect.x, top: rect.y, width: rect.width, height: rect.height }}>
          <span className="size">
            {Math.round(rect.width * pixelRatio)} × {Math.round(rect.height * pixelRatio)}
          </span>
        </div>
      ) : (
        <div className="dim" />
      )}
    </div>
  );
}
