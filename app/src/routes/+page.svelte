<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import TreeRow, { type Row } from "$lib/TreeRow.svelte";
  import Treemap, { type TNode } from "$lib/Treemap.svelte";
  import Icon from "$lib/Icon.svelte";
  import { revealItemInDir } from "@tauri-apps/plugin-opener";
  import { sel, view, joinPath, parentPath, type Mode, type Unit } from "$lib/state.svelte";
  import { fmt } from "$lib/format";

  const modes: { id: Mode; label: string; icon: "chart" | "drive" | "file" }[] = [
    { id: "size", label: "Tamaño", icon: "chart" },
    { id: "alloc", label: "Asignado", icon: "drive" },
    { id: "files", label: "Archivos", icon: "file" },
  ];
  const units: Unit[] = ["auto", "KB", "MB", "GB"];

  let drives = $state<string[]>([]);
  let path = $state("C:\\");
  let root = $state<Row | null>(null);
  let free = $state<number | null>(null);
  let tm = $state<TNode | null>(null);
  let scanning = $state(false);
  let seen = $state(0);
  let error = $state("");
  let secs = $state(0);
  let hint = $state("");
  let tmH = $state(280);
  let admin = $state(false);
  let turbo = $state(true);
  let mode = $state("");
  let note = $state("");
  let scanPath = $state("");
  let toast = $state("");

  async function elevate() {
    try { await invoke("relaunch_admin"); } catch (e) { note = String(e); }
  }

  onMount(() => {
    invoke<boolean>("is_admin").then((a) => (admin = a));
    invoke<string[]>("drives").then((d) => (drives = d));
    const un = listen<number>("scan-progress", (e) => (seen = e.payload));
    return () => { un.then((f) => f()); };
  });

  // El treemap sigue a la seleccion (fila del arbol o clic en el propio treemap).
  $effect(() => {
    const p = $state.snapshot(sel.path), metric = view.mode, base = sel.full;
    if (root) invoke<TNode | null>("treemap", { path: p, metric }).then((t) => { if (t) annotate(t, base); tm = t; });
  });

  /** Pone la ruta completa en cada nodo para poder abrirlo desde el treemap. */
  function annotate(n: TNode, full: string) {
    n.full = full;
    for (const k of n.kids) if (!k.loose) annotate(k, joinPath(full, k.name)); else k.full = full;
  }

  function flash(t: string) {
    toast = t;
    setTimeout(() => (toast = ""), 2200);
  }
  const reveal = () => revealItemInDir(sel.full).catch((e) => flash(String(e)));
  const copyPath = () => navigator.clipboard.writeText(sel.full).then(() => flash("Ruta copiada"), () => flash("No se pudo copiar"));
  const expand = (more: boolean) => { view.level = more ? Math.min(view.level + 1, 5) : 1; view.nonce++; };
  function keys(e: KeyboardEvent) {
    if (e.key === "F5") { e.preventDefault(); if (!scanning) scan(); }
    else if (e.altKey && e.key === "ArrowUp") { e.preventDefault(); if (sel.path.length) up(); }
    else if (e.ctrlKey && e.shiftKey && e.key.toLowerCase() === "c" && root) { e.preventDefault(); copyPath(); }
  }
  const up = () => { sel.path = sel.path.slice(0, -1); sel.full = parentPath(sel.full); };
  const pick = (n: TNode) => { sel.full = n.full ?? sel.full; sel.path = n.path; };

  async function scan() {
    scanning = true; error = ""; note = ""; root = null; tm = null; seen = 0; sel.path = [];
    scanPath = path; sel.full = path; view.level = 1; view.nonce++;
    const t = performance.now();
    try {
      const r = await invoke<{ root: Row; free: number | null; mode: string; note: string | null }>("scan", { path, turbo: turbo && admin });
      free = r.free;
      mode = r.mode;
      note = r.note ?? "";
      root = r.root;
      secs = (performance.now() - t) / 1000;
    } catch (e) {
      error = String(e);
    } finally {
      scanning = false;
    }
  }

  function drag(e: PointerEvent) {
    const y0 = e.clientY, h0 = tmH;
    const move = (m: PointerEvent) => (tmH = Math.min(window.innerHeight - 260, Math.max(140, h0 + y0 - m.clientY)));
    const up = () => { removeEventListener("pointermove", move); removeEventListener("pointerup", up); };
    addEventListener("pointermove", move);
    addEventListener("pointerup", up);
  }
</script>

<svelte:window onkeydown={keys} />

<main>
  <header class="cmd">
    <label class="combo" title="Disco">
      <Icon name="drive" />
      <select bind:value={path} disabled={scanning} aria-label="Disco">
        {#each drives as d}<option value={d}>{d}</option>{/each}
      </select>
    </label>
    <input class="field" bind:value={path} disabled={scanning} aria-label="Ruta" spellcheck="false"
      onkeydown={(e) => e.key === "Enter" && scan()} />
    <button class="btn accent" title="Escanear (F5)" onclick={scan} disabled={scanning}>
      <Icon name="scan" /> {scanning ? "Analizando" : "Escanear"}
    </button>
    <span class="sep"></span>
    {#if admin}
      <label class="switch" title="Lee la MFT de NTFS: mucho más rápido, solo en la raíz de un disco">
        <input type="checkbox" role="switch" bind:checked={turbo} disabled={scanning} />
        <span class="track"><span class="thumb"></span></span>
        Turbo (MFT)
      </label>
    {:else}
      <button class="btn" onclick={elevate} title="El modo turbo necesita permisos de administrador">
        <Icon name="bolt" /><span class="lbl">Habilitar turbo</span>
      </button>
    {/if}
  </header>

  <div class="tools" role="toolbar" aria-label="Herramientas">
    <div class="group">
      <span class="gl">Modo</span>
      <div class="seg" role="radiogroup" aria-label="Modo de vista">
        {#each modes as m}
          <button role="radio" aria-checked={view.mode === m.id} class:on={view.mode === m.id} onclick={() => (view.mode = m.id)}>
            <Icon name={m.icon} size={15} /><span class="lbl">{m.label}</span>
          </button>
        {/each}
      </div>
    </div>
    <span class="vsep"></span>
    <div class="group">
      <span class="gl">Unidad</span>
      <div class="seg" role="radiogroup" aria-label="Unidad">
        {#each units as u}
          <button role="radio" aria-checked={view.unit === u} class:on={view.unit === u} onclick={() => (view.unit = u)}>
            {u === "auto" ? "Auto" : u}
          </button>
        {/each}
      </div>
    </div>
    <span class="vsep"></span>
    <div class="group">
      <span class="gl">Árbol</span>
      <div class="btns">
        <button class="btn subtle sm" title="Expandir un nivel" aria-label="Expandir" disabled={!root} onclick={() => expand(true)}><Icon name="expand" size={15} /><span class="lbl">Expandir</span></button>
        <button class="btn subtle sm" title="Contraer todo" aria-label="Contraer" disabled={!root} onclick={() => expand(false)}><Icon name="collapse" size={15} /><span class="lbl">Contraer</span></button>
      </div>
    </div>
    <span class="vsep"></span>
    <div class="group">
      <span class="gl">Carpeta</span>
      <div class="btns">
        <button class="btn subtle sm" title="Abrir en el Explorador" aria-label="Abrir en Explorador" disabled={!root || !sel.full} onclick={reveal}><Icon name="external" size={15} /><span class="lbl">Abrir en Explorador</span></button>
        <button class="btn subtle sm" title="Copiar ruta (Ctrl+Mayús+C)" aria-label="Copiar ruta" disabled={!root || !sel.full} onclick={copyPath}><Icon name="copy" size={15} /><span class="lbl">Copiar ruta</span></button>
      </div>
    </div>
  </div>

  {#if note}
    <div class="infobar" role="status"><Icon name="info" /> {note}</div>
  {/if}

  <section class="card">
    <div class="progress" class:on={scanning} role="progressbar" aria-hidden={!scanning}><span></span></div>

    <div class="head">
      <span>Nombre</span><span class="num" class:on={view.mode === "size"}>Tamaño</span><span class="num" class:on={view.mode === "alloc"}>Asignado</span>
      <span class="num" class:on={view.mode === "files"}>Archivos</span><span class="num">Carpetas</span><span class="pcth">% del principal</span><span>Modificado</span>
    </div>

    <div class="body" role="tree">
      {#if error}
        <div class="empty"><Icon name="info" size={36} /><p class="t err">{error}</p></div>
      {:else if scanning}
        <div class="empty"><Icon name="scan" size={36} /><p class="t">Analizando el disco</p><p class="s">{seen.toLocaleString()} archivos</p></div>
      {:else if root}
        <TreeRow row={root} path={[]} full={scanPath} parentVal={root[view.mode]} depth={0} />
      {:else}
        <div class="empty"><Icon name="drive" size={36} /><p class="t">Elige un disco y pulsa Escanear</p><p class="s">Verás qué carpetas ocupan más espacio.</p></div>
      {/if}
    </div>

    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <div class="split" role="separator" aria-orientation="horizontal" tabindex="0" onpointerdown={drag}></div>

    <div class="map" style="height:min({tmH}px, 48vh)">
      <div class="maptools">
        <button class="btn subtle sm" title="Subir (Alt+↑)" disabled={!sel.path.length} onclick={up}>
          <Icon name="up" size={14} /> Subir
        </button>
        <span class="hint">{hint}</span>
      </div>
      <div class="canvas"><Treemap data={tm} free={sel.path.length || view.mode === "files" ? null : free} onpick={pick} onhover={(t) => (hint = t)} /></div>
    </div>
  </section>

  <footer class="status">
    {#if root}
      <span>{fmt(root.size)}</span><span>{root.files.toLocaleString()} archivos</span>
      <span>{root.dirs.toLocaleString()} carpetas</span><span>{secs.toFixed(1)} s</span>
      <span class="chip" class:turbo={mode === "mft"}>{mode === "mft" ? "MFT" : "Normal"}</span>
    {:else}<span>Listo</span>{/if}
  </footer>
  {#if toast}<div class="toast" role="status">{toast}</div>{/if}
</main>

<style>
  /* Tokens Fluent (Windows 11). El acento es el del sistema. */
  :global(:root) {
    --text: #1b1b1b; --text2: rgba(0, 0, 0, .62); --text3: rgba(0, 0, 0, .45);
    --card: rgba(255, 255, 255, .72); --stroke: rgba(0, 0, 0, .08); --hover: rgba(0, 0, 0, .04);
    --ctl: rgba(255, 255, 255, .7); --ctl-hover: rgba(255, 255, 255, .9); --track: rgba(0, 0, 0, .08);
    --folder: #c98b00; --accent: #0067c0; --on-accent: #fff;
    color-scheme: light dark;
  }
  @media (prefers-color-scheme: dark) {
    :global(:root) {
      --text: #fff; --text2: rgba(255, 255, 255, .78); --text3: rgba(255, 255, 255, .55);
      --card: rgba(255, 255, 255, .045); --stroke: rgba(255, 255, 255, .08); --hover: rgba(255, 255, 255, .06);
      --ctl: rgba(255, 255, 255, .06); --ctl-hover: rgba(255, 255, 255, .09); --track: rgba(255, 255, 255, .12);
      --folder: #f0b429; --accent: #60cdff; --on-accent: #000;
    }
  }
  @supports (color: AccentColor) {
    :global(:root) { --accent: AccentColor; --on-accent: AccentColorText; }
  }
  :global(:root) { --sel: color-mix(in srgb, var(--accent) 16%, transparent); --cols: minmax(220px, 1fr) 84px 84px 80px 78px 160px 92px; }

  :global(html), :global(body) { background: transparent; }
  :global(body) {
    margin: 0; color: var(--text); overflow: hidden; user-select: none;
    font: 14px/1.4 "Segoe UI Variable Text", "Segoe UI Variable", "Segoe UI", system-ui, sans-serif;
  }
  :global(*) { scrollbar-width: thin; scrollbar-color: var(--track) transparent; }

  main { display: flex; flex-direction: column; height: 100vh; padding: 0 12px; gap: 10px; box-sizing: border-box; }

  /* Barra de comandos */
  .cmd { flex-wrap: wrap; display: flex; align-items: center; gap: 8px; padding-top: 10px; }
  .combo { position: relative; display: flex; align-items: center; gap: 8px; padding: 0 8px 0 10px; height: 32px; border-radius: 6px; background: var(--ctl); border: 1px solid var(--stroke); color: var(--text2); }
  .combo:hover { background: var(--ctl-hover); }
  select { appearance: none; background: none; border: 0; color: var(--text); font: inherit; padding-right: 4px; outline: none; cursor: pointer; }
  select option { background: var(--bg-solid, Canvas); color: CanvasText; }
  .field { flex: 1; min-width: 0; height: 32px; box-sizing: border-box; padding: 0 12px; border-radius: 6px; background: var(--ctl); border: 1px solid var(--stroke); border-bottom-color: var(--text3); color: var(--text); font: inherit; user-select: text; outline: none; }
  .field:focus { border-bottom: 2px solid var(--accent); padding-bottom: 0; }
  .btn { display: inline-flex; align-items: center; gap: 8px; height: 32px; padding: 0 14px; border-radius: 6px; border: 1px solid var(--stroke); background: var(--ctl); color: var(--text); font: inherit; cursor: default; white-space: nowrap; }
  .btn:hover:not(:disabled) { background: var(--ctl-hover); }
  .btn:active:not(:disabled) { opacity: .8; }
  .btn:focus-visible, select:focus-visible, .switch input:focus-visible + .track { outline: 2px solid var(--accent); outline-offset: 2px; }
  .btn:disabled { opacity: .45; }
  .btn.accent { background: var(--accent); border-color: transparent; color: var(--on-accent); font-weight: 600; }
  .btn.accent:hover:not(:disabled) { background: color-mix(in srgb, var(--accent) 88%, var(--on-accent)); }
  .btn.subtle { background: transparent; border-color: transparent; }
  .btn.subtle:hover:not(:disabled) { background: var(--hover); }
  .btn.sm { height: 26px; padding: 0 10px; font-size: 13px; }
  .sep { width: 1px; height: 20px; background: var(--stroke); margin: 0 4px; }

  /* Interruptor */
  .switch { display: inline-flex; align-items: center; gap: 10px; white-space: nowrap; color: var(--text); }
  .switch input { position: absolute; opacity: 0; pointer-events: none; }
  .switch .track { position: relative; width: 40px; height: 20px; border-radius: 10px; border: 1px solid var(--text2); box-sizing: border-box; transition: background .15s; }
  .switch .thumb { position: absolute; left: 4px; top: 4px; width: 10px; height: 10px; border-radius: 50%; background: var(--text2); transition: transform .15s, background .15s; }
  .switch input:checked + .track { background: var(--accent); border-color: var(--accent); }
  .switch input:checked + .track .thumb { transform: translateX(20px); background: var(--on-accent); }
  .switch input:disabled + .track { opacity: .45; }

  /* Barra de herramientas agrupada (como las secciones de la cinta de TreeSize, en estilo Fluent) */
  .tools { display: flex; align-items: stretch; gap: 14px; padding: 2px 4px 0; flex-wrap: wrap; }
  .group { display: flex; flex-direction: column; gap: 4px; }
  .gl { font-size: 11px; color: var(--text3); padding-left: 2px; }
  .btns { display: flex; gap: 2px; }
  .seg { display: inline-flex; padding: 2px; gap: 2px; border-radius: 8px; background: var(--ctl); border: 1px solid var(--stroke); }
  .seg button { display: inline-flex; align-items: center; gap: 6px; height: 26px; padding: 0 10px; border: 0; border-radius: 6px; background: transparent; color: var(--text2); font: inherit; font-size: 13px; cursor: default; }
  .seg button:hover { background: var(--hover); color: var(--text); }
  .seg button.on { background: var(--sel); color: var(--text); font-weight: 600; box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--accent) 35%, transparent); }
  .seg button:focus-visible { outline: 2px solid var(--accent); outline-offset: 1px; }
  .vsep { width: 1px; background: var(--stroke); margin: 4px 0; }
  .head .on { color: var(--text); font-weight: 600; }
  .toast { position: fixed; left: 50%; bottom: 40px; transform: translateX(-50%); padding: 8px 16px; border-radius: 8px; background: var(--text); color: var(--card-solid, Canvas); font-size: 13px; box-shadow: 0 6px 20px rgba(0,0,0,.35); }

  /* Compactado: la tabla pierde columnas por orden de importancia segun el ancho de la tarjeta. */
  @container (max-width: 1000px) {
    .body, .head { --cols: minmax(140px, 1fr) 84px 84px 80px 78px 160px; }
    .head > :nth-child(7), :global(.row > :nth-child(7)) { display: none; }
  }
  @container (max-width: 820px) {
    .body, .head { --cols: minmax(120px, 1fr) 84px 84px 80px 140px; }
    .head > :nth-child(5), :global(.row > :nth-child(5)) { display: none; }
  }
  @container (max-width: 640px) {
    .body, .head { --cols: minmax(100px, 1fr) 80px 120px; }
    .head > :nth-child(3), .head > :nth-child(4), :global(.row > :nth-child(3)), :global(.row > :nth-child(4)) { display: none; }
  }
  /* Solo iconos cuando la ventana es estrecha. */
  @media (max-width: 860px) {
    .lbl { display: none; }
    .btn.sm, .seg button { padding: 0 8px; }
    .tools { gap: 8px; }
  }
  @media (max-width: 640px) {
    .gl { display: none; }
  }

  .infobar { display: flex; align-items: center; gap: 10px; padding: 8px 12px; border-radius: 6px; background: var(--sel); border: 1px solid var(--stroke); color: var(--text); }

  /* Tarjeta de contenido */
  .card { container-type: inline-size; position: relative; flex: 1; min-height: 0; display: flex; flex-direction: column; overflow: hidden; background: var(--card); border: 1px solid var(--stroke); border-radius: 8px; }
  .progress { height: 3px; flex: none; overflow: hidden; opacity: 0; }
  .progress.on { opacity: 1; }
  .progress span { display: block; width: 30%; height: 100%; border-radius: 2px; background: var(--accent); animation: slide 1.2s ease-in-out infinite; }
  @keyframes slide { from { transform: translateX(-100%); } to { transform: translateX(340%); } }

  .head { display: grid; grid-template-columns: var(--cols); gap: 10px; margin: 0 6px; padding: 6px 10px 8px; color: var(--text2); font-size: 12px; border-bottom: 1px solid var(--stroke); }
  .num { text-align: right; }
  .body { flex: 1; min-height: 80px; overflow: auto; padding: 4px 0; scrollbar-gutter: stable; }
  .head { padding-right: 18px; }
  .pcth { padding-left: 16px; }

  .empty { height: 100%; min-height: 160px; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 2px; color: var(--text3); }
  .empty p { margin: 0; }
  .empty .t { margin-top: 10px; color: var(--text); font-weight: 600; }
  .empty .s { color: var(--text2); font-size: 13px; }
  .empty .err { color: #d13438; }

  .split { height: 5px; flex: none; cursor: row-resize; border-top: 1px solid var(--stroke); }
  .split:hover, .split:focus-visible { background: var(--sel); outline: none; }
  .map { display: flex; flex-direction: column; min-height: 0; flex: none; }
  .maptools { display: flex; align-items: center; gap: 10px; padding: 4px 8px; }
  .hint { color: var(--text2); font-size: 13px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .canvas { flex: 1; min-height: 0; padding: 0 8px 8px; }

  /* Barra de estado */
  .status { display: flex; align-items: center; gap: 16px; height: 28px; padding: 0 6px 6px; color: var(--text2); font-size: 12px; }
  .chip { padding: 1px 8px; border-radius: 10px; background: var(--track); }
  .chip.turbo { background: var(--sel); color: var(--text); }
</style>
