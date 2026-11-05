import { useEffect, useRef, useState, type ReactNode } from "react";

interface Props<T> {
  items: T[];
  itemHeight: number;
  /** 보이는 범위 밖으로 미리 그려 둘 줄 수. 빠르게 스크롤해도 빈 칸이 덜 보인다 */
  overscan?: number;
  selectedIndex?: number;
  className?: string;
  render: (item: T, index: number) => ReactNode;
}

/** 높이가 같은 항목 목록. 수백 장짜리 책도 보이는 썸네일만 그린다 */
export function VirtualList<T>({ items, itemHeight, overscan = 4, selectedIndex, className, render }: Props<T>) {
  const ref = useRef<HTMLDivElement>(null);
  const [scrollTop, setScrollTop] = useState(0);
  const [height, setHeight] = useState(600);

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const observer = new ResizeObserver(() => setHeight(el.clientHeight));
    observer.observe(el);
    return () => observer.disconnect();
  }, []);

  // 고른 항목이 화면 밖이면 보이게 굴린다(키보드로 넘길 때)
  useEffect(() => {
    const el = ref.current;
    if (!el || selectedIndex === undefined || selectedIndex < 0) return;
    const top = selectedIndex * itemHeight;
    if (top < el.scrollTop) el.scrollTop = top;
    else if (top + itemHeight > el.scrollTop + el.clientHeight) el.scrollTop = top + itemHeight - el.clientHeight;
  }, [selectedIndex, itemHeight]);

  const first = Math.max(0, Math.floor(scrollTop / itemHeight) - overscan);
  const last = Math.min(items.length, Math.ceil((scrollTop + height) / itemHeight) + overscan);

  return (
    <div ref={ref} className={className} style={{ overflowY: "auto" }} onScroll={(e) => setScrollTop(e.currentTarget.scrollTop)}>
      <div style={{ height: items.length * itemHeight, position: "relative" }}>
        {items.slice(first, last).map((item, i) => (
          <div key={first + i} style={{ position: "absolute", top: (first + i) * itemHeight, left: 0, right: 0, height: itemHeight }}>
            {render(item, first + i)}
          </div>
        ))}
      </div>
    </div>
  );
}
