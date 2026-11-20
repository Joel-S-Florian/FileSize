import { view } from "./state.svelte";

const U = ["B", "KB", "MB", "GB", "TB"];

export function fmt(b: number): string {
  const fixed = U.indexOf(view.unit); // -1 en "auto"
  if (fixed > 0) return `${(b / 1024 ** fixed).toLocaleString("es", { maximumFractionDigits: fixed >= 3 ? 2 : 1 })} ${view.unit}`;
  let i = 0;
  while (b >= 1024 && i < 4) { b /= 1024; i++; }
  return `${b.toFixed(i ? 1 : 0)} ${U[i]}`;
}
export const fmtDate = (s: number) => (s ? new Date(s * 1000).toLocaleDateString("es") : "");
