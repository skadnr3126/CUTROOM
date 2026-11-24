import { useEffect, useMemo, useReducer, useState } from "react";
import { invoke } from "./projectSession";

type Range = { start: number; end: number; animated: boolean };
type Sheet = { first: number; count: number };
function sheetAt(range: Range, frame: number): Sheet {
  if (!range.animated) return {first: range.start, count: 1};
  const first = range.start + Math.floor((frame - range.start) / 15) * 15;
  return {first, count: Math.min(15, range.end - first)};
}

// Scrubbing replaces the pending request. Keep at most one FFmpeg job in flight for this overlay.
class SheetCache {
  images = new Map<number,string>();
  error = "";
  private desired: Sheet[] = [];
  private working = false;
  private source: string;
  private notify: () => void;
  constructor(source: string, notify: () => void) { this.source = source; this.notify = notify; }
  request(current?: Sheet, next?: Sheet) {
    this.desired = [current, next].filter((s): s is Sheet => !!s);
    if (!this.working && !this.error) void this.load();
  }
  private async load() {
    this.working = true;
    try {
      for (;;) {
        const needed = this.desired.find(s => !this.images.has(s.first));
        if (!needed) break;
        const encoded = await invoke<string>("ass_preview_sheet", {source:this.source, ...needed});
        if (!this.desired.some(s => s.first === needed.first)) continue;
        const url = "data:image/png;base64," + encoded;
        const image = new Image();
        image.src = url;
        await image.decode();
        this.images.set(needed.first, url);
        if (this.images.size > 3) {
          const old = [...this.images.keys()].find(k => !this.desired.some(s => s.first === k));
          if (old !== undefined) this.images.delete(old);
        }
        this.notify();
      }
    } catch (e) {
      this.error = String(e);
      this.notify();
    } finally { this.working = false; }
  }
}

export function AssPreview({source, ms}: {source:string; ms:number}) {
  const [,refresh] = useReducer(n => n + 1,0);
  const cache = useMemo(() => new SheetCache(source,refresh),[source]);
  const [plan,setPlan] = useState<{source:string; ranges:Range[]; error?:string}>();
  useEffect(() => {
    let active = true;
    invoke<Range[]>("ass_ranges",{source}).then(ranges => {if(active)setPlan({source,ranges});})
      .catch(e => {if(active)setPlan({source,ranges:[],error:String(e)});});
    return () => {active=false;cache.request();};
  },[source,cache]);
  const ranges = plan?.source === source ? plan.ranges : [];
  const frame = Math.max(0,Math.floor(ms * 30 / 1000));
  const index = ranges.findIndex(r => frame < r.end);
  const range = ranges[index];
  const visible = range && frame >= range.start;
  const current = visible ? sheetAt(range,frame) : undefined;
  const nextFrame = current && range.animated ? current.first + current.count : range?.end;
  const nextRange = !visible ? range : nextFrame < range.end ? range : ranges[index + 1];
  const next = nextRange ? sheetAt(nextRange, Math.max(nextRange.start, visible ? nextFrame : nextRange.start)) : undefined;
  useEffect(() => {
    cache.request(current,next);
  },[cache,current?.first,current?.count,next?.first,next?.count]);
  const error = cache.error || (plan?.source === source && plan.error);
  if(error)return <span role="alert" className="preview-error">{error}</span>;
  const url = current && cache.images.get(current.first);
  if(!current || !url)return null;
  const cols = Math.min(5,current.count), rows = Math.ceil(current.count / cols);
  const i = current.count === 1 ? 0 : frame - current.first;
  return <div role="img" aria-label="ASS 자막 미리보기" style={{
    position:"absolute",inset:0,pointerEvents:"none",backgroundImage:"url("+url+")",
    backgroundSize:cols*100+"% "+rows*100+"%",backgroundRepeat:"no-repeat",
    backgroundPosition:(cols===1?0:(i%cols)/(cols-1)*100)+"% "+(rows===1?0:Math.floor(i/cols)/(rows-1)*100)+"%"
  }}/>;
}

