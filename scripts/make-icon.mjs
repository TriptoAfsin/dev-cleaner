// Rasterise app-icon.svg to the 1024px PNG that `tauri icon` expects.
import { readFileSync, writeFileSync } from "node:fs"
import { Resvg } from "@resvg/resvg-js"

const svg = readFileSync(new URL("../app-icon.svg", import.meta.url))
const png = new Resvg(svg, { fitTo: { mode: "width", value: 1024 } }).render().asPng()
writeFileSync(new URL("../app-icon.png", import.meta.url), png)
console.log("app-icon.png written")
