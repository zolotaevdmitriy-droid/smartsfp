import { writable } from "svelte/store";
import { api, type StatusReport, type SystemSnapshot } from "./api";

/** Live status from cryptod, polled by the App component. */
export const status = writable<StatusReport | null>(null);
/** Last status fetch error (network / cryptod down). null when healthy. */
export const statusError = writable<string | null>(null);

/** Live system snapshot (CPU/RAM/disk/temp/network). */
export const sysmon = writable<SystemSnapshot | null>(null);

/** Drift-aware poller. Stop returns a cancel function. */
export function startStatusPolling(intervalMs = 2000) {
  let stopped = false;
  let timer: ReturnType<typeof setTimeout> | null = null;

  async function tick() {
    if (stopped) return;
    // Two parallel requests per tick; system one is allowed to fail
    // independently (cryptod could be down but module is alive).
    const [stRes, sysRes] = await Promise.allSettled([api.status(), api.system()]);
    if (stRes.status === "fulfilled") {
      status.set(stRes.value);
      statusError.set(null);
    } else {
      const e = stRes.reason as any;
      statusError.set(e?.error ?? e?.message ?? String(e));
    }
    if (sysRes.status === "fulfilled") {
      sysmon.set(sysRes.value);
    }
    if (!stopped) timer = setTimeout(tick, intervalMs);
  }
  tick();

  return () => {
    stopped = true;
    if (timer) clearTimeout(timer);
  };
}

// ---- Toast notifications --------------------------------------------------

export interface Toast {
  id: number;
  level: "ok" | "warn" | "err";
  text: string;
}

let toastSeq = 0;
export const toasts = writable<Toast[]>([]);

export function toast(level: Toast["level"], text: string, autohideMs = 4500) {
  const id = ++toastSeq;
  toasts.update((arr) => [...arr, { id, level, text }]);
  if (autohideMs > 0) {
    setTimeout(() => {
      toasts.update((arr) => arr.filter((t) => t.id !== id));
    }, autohideMs);
  }
}

// ---- Hash-based router (tiny, no dep) -------------------------------------

export const route = writable<string>(currentRoute());

function currentRoute(): string {
  const h = (typeof location !== "undefined" ? location.hash : "") || "#/dashboard";
  return h.replace(/^#\//, "").split("?")[0].split("/")[0] || "dashboard";
}

/** Register hashchange handler. Call from App.svelte's onMount so the
 * listener is bound after the DOM is ready (more reliable than a
 * module-init side effect). */
export function installRouter() {
  if (typeof window === "undefined") return () => {};
  if (!location.hash) location.hash = "#/dashboard";
  const handler = () => route.set(currentRoute());
  window.addEventListener("hashchange", handler);
  // Ensure store reflects current hash on install (in case App mounted
  // after some scripts already set the hash).
  route.set(currentRoute());
  return () => window.removeEventListener("hashchange", handler);
}

export function navigate(name: string) {
  // Belt-and-braces: hashchange listener will normally update the store,
  // but we also set it directly. Handles cases where the browser doesn't
  // fire hashchange (e.g. navigating to the same hash) and removes a
  // class of obscure bugs where the listener didn't attach in time.
  if (typeof location !== "undefined") {
    location.hash = "#/" + name;
  }
  route.set(name);
}

// ---- Format helpers -------------------------------------------------------

export function fmtDuration(sec: number): string {
  const d = Math.floor(sec / 86400);
  const h = Math.floor((sec % 86400) / 3600);
  const m = Math.floor((sec % 3600) / 60);
  const s = sec % 60;
  if (d > 0) return `${d}д ${h}ч ${m}м`;
  if (h > 0) return `${h}ч ${m}м ${s}с`;
  if (m > 0) return `${m}м ${s}с`;
  return `${s}с`;
}

export function fmtN(n: number): string {
  return new Intl.NumberFormat("ru-RU").format(n);
}

export function fmtBytes(n: number): string {
  if (n < 1024) return n + " B";
  const units = ["КБ", "МБ", "ГБ", "ТБ"];
  let i = -1;
  do {
    n /= 1024;
    i++;
  } while (n >= 1024 && i < units.length - 1);
  return n.toFixed(n < 10 ? 1 : 0) + " " + units[i];
}

export function fmtBps(bps: number): string {
  if (bps < 1000) return bps.toFixed(0) + " бит/с";
  const units = ["Кбит/с", "Мбит/с", "Гбит/с"];
  let v = bps / 1000;
  let i = 0;
  while (v >= 1000 && i < units.length - 1) {
    v /= 1000;
    i++;
  }
  return v.toFixed(v < 10 ? 2 : 1) + " " + units[i];
}
