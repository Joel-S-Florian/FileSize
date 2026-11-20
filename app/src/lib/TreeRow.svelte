<script lang="ts" module>
  export type Row = {
    name: string; size: number; alloc: number; files: number; dirs: number; mtime: number;
    has_children: boolean; idx: number | null;
  };
</script>

<script lang="ts">
  import { untrack } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { fmt, fmtDate } from "./format";
  import { sel, view, metric, keyOf, joinPath } from "./state.svelte";
  import TreeRow from "./TreeRow.svelte";
  import Icon from "./Icon.svelte";

  let { row, path, full, parentVal, depth }: { row: Row; path: number[]; full: string; parentVal: number; depth: number } = $props();

  let open = $state(false);
  let kids = $state<Row[] | null>(null);
  const isFiles = $derived(row.idx === null && depth > 0);
  const expandable = $derived(row.has_children && !isFiles);
  const selected = $derived(!isFiles && keyOf(sel.path) === keyOf(path));
  const sorted = $derived(kids ? [...kids].sort((a, b) => metric(b) - metric(a)) : []);

  async function setOpen(want: boolean) {
    open = want && expandable;
    if (open && !kids) kids = await invoke<Row[]>("children", { path });
  }

  async function click() {
    if (isFiles) return;
    sel.full = full;
    sel.path = path;
    if (expandable) await setOpen(!open);
  }

  // Expandir/Contraer de la barra de herramientas (y apertura inicial de la raiz).
  $effect(() => {
    view.nonce;
    const want = depth < view.level;
    untrack(() => setOpen(want));
  });

  const pct = $derived(parentVal ? (metric(row) * 100) / parentVal : 0);
</script>

<div class="row" class:selected role="treeitem" aria-selected={selected} aria-expanded={expandable ? open : undefined}
  tabindex="0" onclick={click} onkeydown={(e) => (e.key === "Enter" || e.key === " ") && click()}>
  <span class="name" class:files={isFiles} style="padding-left:{depth * 18}px">
    <span class="chev">{#if expandable}<Icon name={open ? "down" : "right"} size={12} />{/if}</span>
    <span class="ico"><Icon name={isFiles ? "file" : "folder"} size={16} /></span>
    <span class="txt">{row.name}</span>
  </span>
  <span class="num" class:on={view.mode === "size"}>{fmt(row.size)}</span>
  <span class="num" class:on={view.mode === "alloc"}>{fmt(row.alloc)}</span>
  <span class="num" class:on={view.mode === "files"}>{row.files.toLocaleString()}</span>
  <span class="num">{isFiles ? "" : row.dirs.toLocaleString()}</span>
  <span class="pct"><i class="val">{pct.toFixed(1)}%</i><span class="track"><span class="fill" style="width:{pct}%"></span></span></span>
  <span class="date">{fmtDate(row.mtime)}</span>
</div>
{#if open && kids}
  {#each sorted as k, i (k.idx ?? "f" + i)}
    <TreeRow row={k} path={k.idx === null ? path : [...path, k.idx]} full={k.idx === null ? full : joinPath(full, k.name)}
      parentVal={metric(row)} depth={depth + 1} />
  {/each}
{/if}

<style>
  .row {
    position: relative; display: grid; grid-template-columns: var(--cols); align-items: center; gap: 10px;
    height: 34px; margin: 0 6px; padding: 0 10px; border-radius: 6px; cursor: default; outline: none;
  }
  .row:hover { background: var(--hover); }
  .row.selected { background: var(--sel); }
  .row.selected::before {
    content: ""; position: absolute; left: 0; top: 9px; bottom: 9px; width: 3px; border-radius: 2px; background: var(--accent);
  }
  .row:focus-visible { outline: 2px solid var(--accent); outline-offset: -2px; }
  .name { display: flex; align-items: center; gap: 6px; min-width: 0; }
  .chev { display: inline-flex; width: 14px; justify-content: center; color: var(--text3); flex: none; }
  .ico { display: inline-flex; color: var(--folder); flex: none; }
  .ico :global(svg) { fill: color-mix(in srgb, currentColor 35%, transparent); }
  .files .ico { color: var(--text3); }
  .files .ico :global(svg) { fill: none; }
  .files .txt { color: var(--text3); }
  .txt { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .num { text-align: right; font-variant-numeric: tabular-nums; color: var(--text2); }
  .num.on { color: var(--text); font-weight: 600; }
  .date { color: var(--text2); font-variant-numeric: tabular-nums; }
  .pct { display: flex; align-items: center; gap: 8px; }
  .val { font-style: normal; width: 42px; text-align: right; font-variant-numeric: tabular-nums; color: var(--text2); }
  .track { flex: 1; height: 6px; border-radius: 3px; background: var(--track); overflow: hidden; }
  .fill { display: block; height: 100%; border-radius: 3px; background: var(--accent); min-width: 2px; }
</style>
