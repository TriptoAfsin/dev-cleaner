// Turn a piece of the UI into a PNG, then copy it or save it.
import { toPng } from "html-to-image"
import { save } from "@tauri-apps/plugin-dialog"
import { api } from "./api"

export async function capture(node: HTMLElement): Promise<string> {
  const bg = getComputedStyle(document.documentElement).getPropertyValue("--bg").trim() || "#ffffff"
  // Fonts are inlined so the image matches the app; 2x for crisp text.
  return toPng(node, { backgroundColor: bg, pixelRatio: 2, cacheBust: true })
}

export async function copyPng(dataUrl: string): Promise<void> {
  const blob = await (await fetch(dataUrl)).blob()
  await navigator.clipboard.write([new ClipboardItem({ "image/png": blob })])
}

/** Ask where to save, then write. Returns the path, or null if cancelled. */
export async function savePng(dataUrl: string, name: string): Promise<string | null> {
  const path = await save({ defaultPath: `${name}.png`, filters: [{ name: "PNG image", extensions: ["png"] }] })
  if (!path) return null
  await api.saveFile(path, dataUrl.split(",")[1] ?? "")
  return path
}

export const stamp = () => new Date().toISOString().slice(0, 10)
