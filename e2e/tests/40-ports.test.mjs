// Ports: a real listener shows as In use with its process, search finds it,
// and freeing it through the UI ends exactly that process.
import { test, before, after } from "node:test"
import assert from "node:assert/strict"
import { spawn } from "node:child_process"
import { freePort, killTree, launch } from "../lib/harness.mjs"
import { sleep } from "../lib/webdriver.mjs"

let ctx
let server
let port

before(async () => {
  port = await freePort()
  server = spawn(process.execPath, ["-e", `require('net').createServer().listen(${port}, '127.0.0.1'); setInterval(() => {}, 1000)`], { stdio: "ignore" })
  await sleep(600)
  ctx = await launch()
  await ctx.settings({ extraPorts: [port] })
})
after(async () => {
  killTree(server?.pid)
  await ctx?.stop()
})

const portRow = () => ctx.s.exec(`return document.querySelector('[data-testid="port-${port}"]')?.innerText ?? ''`)

test("the backend reports the listener with its PID", async () => {
  const list = await ctx.s.invoke("list_ports", { all: false, extra: [port] })
  const e = list.find((p) => p.port === port)
  assert.equal(e.state, "inUse")
  assert.equal(e.pid, server.pid)
  assert.match(e.process, /node/i)
})

test("common dev ports that nothing holds show as Free", async () => {
  const list = await ctx.s.invoke("list_ports", { all: false, extra: [] })
  assert.ok(list.some((p) => p.state === "free"))
})

test("the Ports page shows it as In use and search narrows to it", async () => {
  await ctx.nav("ports")
  await ctx.s.waitFor(async () => /In use[\s\S]*node/i.test(await portRow()), "port row in use")
  const input = await ctx.tid("port-search")
  await input.click()
  await ctx.s.type(String(port))
  await ctx.s.waitFor(async () => (await ctx.s.exec(`return document.querySelectorAll('[data-testid=port-list] li').length`)) === 1, "search narrows to one row")
})

test("selecting the row fills the inspector with the process details", async () => {
  await (await ctx.tid(`port-${port}`)).click()
  await ctx.s.waitFor(async () => new RegExp(`Port ${port}[\\s\\S]*PID[\\s\\S]*${server.pid}`, "i").test(await ctx.s.exec(`return document.querySelector('[data-testid=port-inspector]').innerText`)), "inspector")
})

test("Free asks first, then ends the process and the port turns Free", async () => {
  await ctx.click("free-port")
  await ctx.s.waitFor(() => ctx.s.byText("[role=dialog]", `Frees port ${port}`), "confirm dialog")
  await ctx.click("confirm-kill")
  await ctx.s.waitFor(async () => server.exitCode !== null || server.signalCode !== null, "server process ended", { timeout: 8000 })
  await ctx.s.waitFor(async () => /Free/.test(await portRow()), "port row turns Free", { timeout: 8000 })
})
