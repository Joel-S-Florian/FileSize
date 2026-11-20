<script lang="ts" module>
  export type TNode = { name: string; size: number; path: number[]; loose: boolean; kids: TNode[]; full?: string };
</script>

<script lang="ts">
  import { fmt } from "./format";
  import { view } from "./state.svelte";

  let { data, free, onpick, onhover }: {
    data: TNode | null; free: number | null; onpick: (n: TNode) => void; onhover: (t: string) => void;
  } = $props();
  // En modo "archivos" el tamano de cada nodo es una cantidad, no bytes.
  const val = (n: number) => (view.mode === "files" ? `${n.toLocaleString()} arch.` : fmt(n));

  type R = { x: number; y: number; w: number; h: number };
  type Hit = { r: R; n: TNode | "free" };

  let wrap: HTMLDivElement;
  let canvas: HTMLCanvasElement;
  let box = $state({ w: 0, h: 0 });
  let hits: Hit[] = [];
  let hover = $state<TNode | "free" | null>(null);

  $effect(() => {
    const ro = new ResizeObserver(() => (box = { w: wrap.clientWidth, h: wrap.clientHeight }));
    ro.observe(wrap);
    return () => ro.disconnect();
  });

  // Squarified treemap (Bruls et al.); sizes ya vienen ordenados de mayor a menor.
  function squarify(sizes: number[], r: R): R[] {
    const total = sizes.reduce((a, b) => a + b, 0);
    const out: R[] = [];
    if (total <= 0 || r.w <= 0 || r.h <= 0) return sizes.map(() => ({ x: r.x, y: r.y, w: 0, h: 0 }));
    const k = (r.w * r.h) / total;
    let { x, y, w, h } = r;
    let i = 0;
    while (i < sizes.length) {
      const side = Math.min(w, h);
      let j = i, sum = 0, worst = Infinity;
      while (j < sizes.length) {
        const s2 = sum + sizes[j] * k;
        let mx = 0, mn = Infinity;
        for (let m = i; m <= j; m++) { const a = sizes[m] * k; mx = Math.max(mx, a); mn = Math.min(mn, a); }
        const w2 = Math.max((side * side * mx) / (s2 * s2), (s2 * s2) / (side * side * mn));
        if (w2 > worst) break;
        worst = w2; sum = s2; j++;
      }
      const thick = sum / side;
      let off = 0;
      for (let m = i; m < j; m++) {
        const len = (sizes[m] * k) / thick;
        out.push(w >= h ? { x, y: y + off, w: thick, h: len } : { x: x + off, y, w: len, h: thick });
        off += len;
      }
      if (w >= h) { x += thick; w -= thick; } else { y += thick; h -= thick; }
      i = j;
    }
    return out;
  }

  const dark = () => matchMedia("(prefers-color-scheme: dark)").matches;
  let theme = $state(0); // sube cuando cambia el tema del sistema, para repintar
  $effect(() => {
    const mq = matchMedia("(prefers-color-scheme: dark)");
    const f = () => theme++;
    mq.addEventListener("change", f);
    return () => mq.removeEventListener("change", f);
  });

  /** Matiz y saturacion del color de acento del sistema (el canvas no entiende `AccentColor`). */
  function accentHS(): [number, number] {
    const el = document.createElement("span");
    el.style.color = "AccentColor";
    document.body.appendChild(el);
    const m = getComputedStyle(el).color.match(/\d+(\.\d+)?/g)?.map(Number) ?? [0, 103, 192];
    el.remove();
    const [r, g, b] = [m[0] / 255, m[1] / 255, m[2] / 255];
    const mx = Math.max(r, g, b), mn = Math.min(r, g, b), d = mx - mn;
    if (!d) return [214, 0.4];
    const h = mx === r ? ((g - b) / d) % 6 : mx === g ? (b - r) / d + 2 : (r - g) / d + 4;
    const l = (mx + mn) / 2;
    return [(h * 60 + 360) % 360, d / (1 - Math.abs(2 * l - 1))];
  }

  function draw() {
    if (!canvas || !box.w || !box.h) return;
    const dpr = devicePixelRatio || 1;
    canvas.width = box.w * dpr; canvas.height = box.h * dpr;
    const c = canvas.getContext("2d")!;
    c.scale(dpr, dpr);
    c.clearRect(0, 0, box.w, box.h);
    hits = [];
    if (!data) return;
    c.font = '12px "Segoe UI Variable","Segoe UI",system-ui,sans-serif';
    c.textBaseline = "middle";
    const d = dark();
    const [hue, sat] = accentHS();
    const s = Math.round(Math.min(0.75, Math.max(0.4, sat)) * 100);
    const fillFor = (depth: number, loose: boolean) =>
      `hsl(${hue} ${loose ? 18 : s}% ${Math.min(64, (d ? 26 : 30) + depth * 7) + (loose ? 6 : 0)}%)`;

    function label(t: string, x: number, y: number, maxW: number) {
      c.fillStyle = "#fff";
      let s = t;
      while (s.length > 1 && c.measureText(s).width > maxW) s = s.slice(0, -2);
      if (s.length < t.length) s += "…";
      c.fillText(s, x, y);
    }

    function node(n: TNode, r: R, depth: number) {
      if (r.w < 3 || r.h < 3) return;
      c.fillStyle = fillFor(depth, n.loose);
      c.fillRect(r.x, r.y, r.w, r.h);
      c.strokeStyle = d ? "rgba(0,0,0,.4)" : "rgba(255,255,255,.6)"; c.lineWidth = 1;
      c.strokeRect(r.x + 0.5, r.y + 0.5, r.w - 1, r.h - 1);
      hits.push({ r, n });
      const head = r.h > 24 && r.w > 50 ? 16 : 0;
      if (head || n.kids.length === 0) {
        if (r.w > 30 && r.h > 14) label(`${n.name} (${val(n.size)})`, r.x + 4, r.y + 9, r.w - 8);
      }
      if (!n.kids.length || !head) return;
      const inner = { x: r.x + 2, y: r.y + head, w: r.w - 4, h: r.h - head - 2 };
      const rs = squarify(n.kids.map((k) => k.size), inner);
      n.kids.forEach((k, i) => node(k, rs[i], depth + 1));
    }

    let area: R = { x: 0, y: 0, w: box.w, h: box.h };
    if (free != null && data.size + free > 0) {
      const fw = Math.max(60, (box.w * free) / (data.size + free));
      area = { x: 0, y: 0, w: box.w - fw, h: box.h };
      const fr = { x: box.w - fw, y: 0, w: fw, h: box.h };
      c.fillStyle = d ? "rgba(255,255,255,.07)" : "rgba(0,0,0,.06)"; c.fillRect(fr.x + 2, fr.y, fr.w - 2, fr.h);
      hits.push({ r: fr, n: "free" });
      c.fillStyle = d ? "rgba(255,255,255,.7)" : "rgba(0,0,0,.6)";
      c.fillText(fr.w >= 120 ? `Libre (${fmt(free)})` : "Libre", fr.x + 10, 14, fr.w - 16);
      area = { ...area, w: area.w - 2 };
    }
    node(data, area, 0);
    if (hover) {
      const h = hits.find((x) => x.n === hover);
      if (h) { c.strokeStyle = "#fbbf24"; c.lineWidth = 2; c.strokeRect(h.r.x + 1, h.r.y + 1, h.r.w - 2, h.r.h - 2); }
    }
  }

  $effect(() => { data; free; box; hover; theme; view.unit; view.mode; draw(); });

  function hitAt(e: MouseEvent) {
    const b = canvas.getBoundingClientRect();
    const x = e.clientX - b.left, y = e.clientY - b.top;
    for (let i = hits.length - 1; i >= 0; i--) {
      const { r, n } = hits[i];
      if (x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h) return n;
    }
    return null;
  }
  function move(e: MouseEvent) {
    const n = hitAt(e);
    hover = n;
    onhover(n === null ? "" : n === "free" ? `Espacio libre: ${fmt(free ?? 0)}` : `${n.name} · ${val(n.size)}`);
  }
  function click(e: MouseEvent) {
    const n = hitAt(e);
    if (n && n !== "free" && !n.loose) onpick(n);
  }
</script>

<div class="tm" bind:this={wrap}>
  <canvas bind:this={canvas} style="width:{box.w}px;height:{box.h}px" onmousemove={move}
    onmouseleave={() => { hover = null; onhover(""); }} onclick={click} aria-label="Mapa de árbol"></canvas>
</div>

<style>
  .tm { position: relative; width: 100%; height: 100%; overflow: hidden; border-radius: 6px; }
  /* absoluto: el tamano del canvas no debe impedir que el contenedor se achique */
  canvas { position: absolute; inset: 0; display: block; cursor: pointer; }
</style>
