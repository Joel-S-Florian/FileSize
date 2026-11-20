// Estado compartido entre el arbol, el treemap y la pagina.
export type Mode = "size" | "alloc" | "files";
export type Unit = "auto" | "KB" | "MB" | "GB";

/** `full` = ruta completa en disco de la seleccion (para abrir/copiar). */
export const sel = $state({ path: [] as number[], full: "" });
export const keyOf = (p: number[]) => p.join(",");

/** `level`/`nonce`: cada cambio de nonce hace que cada fila abra o cierre segun su profundidad. */
export const view = $state({ mode: "size" as Mode, unit: "auto" as Unit, level: 1, nonce: 1 });

export const metric = (r: { size: number; alloc: number; files: number }) => r[view.mode];
export const joinPath = (base: string, name: string) => base.replace(/\\$/, "") + "\\" + name;
export const parentPath = (p: string) => {
  const q = p.replace(/\\[^\\]+$/, "");
  return /^[A-Za-z]:$/.test(q) ? q + "\\" : q;
};
