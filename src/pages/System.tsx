import { useEffect, useState, type ReactNode } from "react"
import { motion } from "motion/react"
import { BatteryCharging, BatteryFull, Copy, HardDrive, RefreshCw, ShieldCheck } from "lucide-react"
import { api, type SystemInfo, type Tool } from "@/lib/api"
import { bytes, cn, duration } from "@/lib/format"
import { BrandIcon } from "@/lib/brands"
import { insights } from "@/lib/insights"
import { usePoll, useStore } from "@/lib/store"
import { Badge, Bar, Button, Card, IconButton, InfoTip, PageHeader, Skeleton } from "@/components/ui"
import { InsightsCard } from "@/components/Insights"
import { SnapshotButton } from "@/components/Share"
import { RelaunchDialog } from "@/components/Privileges"
import type { Page } from "@/components/Shell"

const TIPS: Record<string, string> = {
  compression:
    "Windows compresses memory that apps haven't touched lately instead of writing it to the page file. It saves RAM for a little CPU. The “Memory Compression” entry in Task Manager is this store, not an app. On macOS it is always on.",
  pagefile: "Disk space the OS uses as overflow when RAM runs out (the page file on Windows, swap on macOS). “Used” is how much is actually holding pages right now.",
  devmode: "Lets you create symlinks without admin rights and sideload apps. pnpm, Bazel and some build tools rely on it on Windows.",
  longpaths: "Allows file paths longer than 260 characters. Deep node_modules trees need it, or installs and deletes fail.",
  devdrive: "A ReFS volume with Defender in performance mode. Package installs and builds run noticeably faster on it.",
  power: "The active power plan. Power saver holds the CPU back; Balanced is right for most machines.",
  threads: "Physical cores run work; threads (hyper-threading / SMT) let each core juggle two tasks.",
  wsl: "Windows Subsystem for Linux. A Running distro holds memory (vmmem) until you stop it with wsl --shutdown.",
  vm: "Memory held right now by the WSL / Docker virtual machine. It often keeps more than it currently needs.",
  admin: "Some actions (freeing ports held by services, trimming service memory) need an elevated app.",
  ai: "Coding agents and CLIs found on your PATH. Versions come from each tool's --version.",
  editors: "Editors found on PATH or in their usual install folders.",
  runtimes: "Languages and tools found on your PATH, with the version that runs when you type its name.",
  uptime: "Time since the last restart. Long uptimes leave more memory in caches and updates pending.",
  health: "How much charge the battery holds now compared with when it was new. Below 80% counts as worn.",
  drives: "Every mounted drive. SSD or HDD as reported by the OS.",
}

// System details take a few seconds to gather; keep the last result so
// coming back to the page is instant while it refreshes.
let last: SystemInfo | null = null

export function SystemPage({ go }: { go: (p: Page) => void }) {
  const info = usePoll(api.systemInfo, 60_000)
  const procs = usePoll(api.listProcesses, 5000)
  const { toast, stats, caches } = useStore()
  const [s, setS] = useState<SystemInfo | null>(last)
  useEffect(() => {
    if (info.data) setS((last = info.data))
  }, [info.data])
  const tips = insights(s, stats, caches, procs.data)
  const [tab, setTab] = useState<"details" | "tips">("details")
  const warnings = tips.filter((t) => t.level === "warn").length

  const copy = async () => {
    if (!s) return
    await navigator.clipboard.writeText(markdown(s))
    toast({ kind: "ok", title: "Copied system report", detail: "Markdown, ready to paste into an issue." })
  }

  return (
    <div className="flex flex-col gap-4">
      <PageHeader
        title="System"
        lede={s ? `${s.osVersion || s.osName} · ${s.hostname} · up ${duration(s.uptime)}` : "Reading system details…"}
        actions={
          <>
            <Button icon={<Copy size={15} />} onClick={copy} disabled={!s}>
              Copy as Markdown
            </Button>
            <SnapshotButton name="system" />
            <IconButton label="Refresh" onClick={() => void info.refresh()}>
              <RefreshCw size={14} />
            </IconButton>
          </>
        }
      />

      <div className="flex w-fit gap-0.5 rounded-xl border border-border bg-surface p-1" role="tablist" aria-label="System view">
        {(
          [
            ["details", "Details", null],
            ["tips", "Recommendations", tips.filter((t) => t.level !== "ok").length || null],
          ] as const
        ).map(([id, label, count]) => (
          <button
            key={id}
            type="button"
            role="tab"
            aria-selected={tab === id}
            data-testid={`system-tab-${id}`}
            onClick={() => setTab(id)}
            className={cn(
              "relative flex h-8 items-center gap-2 rounded-lg px-3.5 text-[13px] font-medium transition-colors",
              tab === id ? "text-on-accent" : "text-muted hover:text-text",
            )}
          >
            {tab === id && <motion.span layoutId="system-tab" className="absolute inset-0 rounded-lg bg-accent" transition={{ type: "spring", stiffness: 500, damping: 40 }} />}
            <span className="relative">{label}</span>
            {count != null && (
              <span className={cn("mono relative rounded-full px-1.5 text-[11px]", tab === id ? "bg-white/20" : warnings ? "bg-warn-soft text-warn" : "bg-track")}>{count}</span>
            )}
          </button>
        ))}
      </div>

      {tab === "tips" ? (
        <InsightsCard items={tips} go={go} loading={!s} />
      ) : (
      <>
      <div className="grid grid-cols-3 items-start gap-3.5">
        <Section title="Processor" sub={s?.arch}>
          <KV k="Model" v={s?.cpuBrand} wrap />
          <KV k="Cores / threads" tip={TIPS.threads} v={s && `${s.cpuCores} / ${s.cpuThreads}`} />
          <KV k="Load now" v={stats && `${stats.cpu.toFixed(0)}%`} />
          <KV k="Graphics" v={s && (s.gpus.length ? s.gpus.join(" + ") : "—")} wrap />
        </Section>
        <Section title="Memory" sub={s && `${bytes(s.memTotal, 1)} installed`}>
          <KV k="In use" v={stats && `${bytes(stats.memUsed)} · ${Math.round((stats.memUsed / stats.memTotal) * 100)}%`} />
          <KV k="Memory compression" tip={TIPS.compression} v={s && (s.compression == null ? "—" : <Badge tone={s.compression ? "ok" : "muted"}>{s.compression ? "On" : "Off"}</Badge>)} />
          {(stats?.compressed ?? s?.compressedBytes) != null && <KV k="Compressed now" v={bytes(stats?.compressed ?? s?.compressedBytes)} />}
          {s?.vmMemory != null && <KV k="WSL / Docker VM" tip={TIPS.vm} v={bytes(s.vmMemory)} />}
        </Section>
        <Section title={s?.osName || "Operating system"} sub={s?.kernel && `kernel ${s.kernel}`}>
          <KV k="Version" v={s?.osVersion} wrap />
          {s?.developerMode != null && <KV k="Developer Mode" tip={TIPS.devmode} v={<Badge tone={s.developerMode ? "ok" : "muted"}>{s.developerMode ? "On" : "Off"}</Badge>} />}
          {s?.longPaths != null && <KV k="Long paths" tip={TIPS.longpaths} v={<Badge tone={s.longPaths ? "ok" : "warn"}>{s.longPaths ? "Enabled" : "Disabled"}</Badge>} />}
          {s?.devDrive != null && <KV k="Dev Drive" tip={TIPS.devdrive} v={<Badge tone={s.devDrive ? "ok" : "muted"}>{s.devDrive ? "Yes" : "None"}</Badge>} />}
          {s?.powerPlan && <KV k="Power plan" tip={TIPS.power} v={s.powerPlan} />}
          <KV k="Uptime" tip={TIPS.uptime} v={s && duration(s.uptime)} />
          {s && /windows/i.test(s.osName) && <KV k="Running as admin" tip={TIPS.admin} v={<AdminValue admin={s.isAdmin} />} />}
        </Section>
      </div>

      <div className="grid grid-cols-3 items-start gap-3.5">
        {/* Left column stacks so short cards never leave holes. */}
        <div className="flex min-w-0 flex-col gap-3.5">
          <PageFileSection s={s} />
          {s?.battery && <BatterySection s={s} />}
          {(!s || s.wsl.length > 0 || s.wslVersion) && (
            <Section title="WSL" sub={s?.wslVersion ? `version ${s.wslVersion}` : undefined} tip={TIPS.wsl}>
              {s ? (
                s.wsl.length ? (
                  s.wsl.map((d) => (
                    <KV
                      key={d.name}
                      k={
                        <span className="flex items-center gap-1.5">
                          <BrandIcon name={d.name.toLowerCase().startsWith("ubuntu") ? "ubuntu" : d.name.includes("docker") ? "docker" : "linux"} size={13} />
                          {d.name}
                          {d.default && <span className="text-[11px] text-subtle">default</span>}
                        </span>
                      }
                      v={<Badge tone={d.state.toLowerCase() === "running" ? "accent" : "muted"}>{d.state}</Badge>}
                    />
                  ))
                ) : (
                  <div className="py-2 text-[12.5px] text-muted">No distros installed.</div>
                )
              ) : (
                <Loading />
              )}
            </Section>
          )}
          <ToolSection title="Editors" tip={TIPS.editors} tools={s?.editors} />
        </div>
        <div className="col-span-2 flex min-w-0 flex-col gap-3.5">
          <DrivesSection s={s} />
          <ToolSection title="Runtimes and tools" tip={TIPS.runtimes} tools={s?.runtimes} cols={2} />
          <ToolSection title="AI coding CLIs and agents" tip={TIPS.ai} tools={s?.aiTools} cols={2} />
        </div>
      </div>
      </>
      )}
    </div>
  )
}

function PageFileSection({ s }: { s: SystemInfo | null }) {
  const files = s?.pageFiles ?? []
  return (
    <Section title={files.length ? "Page file" : "Swap"} tip={TIPS.pagefile} sub={s && (files.length ? `${files.length} file${files.length === 1 ? "" : "s"}` : bytes(s.swapTotal))}>
      {!s ? (
        <Loading />
      ) : files.length ? (
        files.map((p) => (
          <div key={p.path} className="flex flex-col gap-1.5 border-t border-border py-2 text-[12.5px] first-of-type:border-t-0">
            <div className="flex justify-between gap-2">
              <span className="mono truncate text-muted">{p.path}</span>
              <span className="mono shrink-0">
                {bytes(p.used)} / {bytes(p.size, 0)}
              </span>
            </div>
            <Bar value={p.size ? p.used / p.size : 0} thin />
          </div>
        ))
      ) : (
        <div className="flex flex-col gap-1.5 py-2 text-[12.5px]">
          <div className="mono flex justify-between">
            <span className="text-muted">Used</span>
            {bytes(s.swapUsed)} / {bytes(s.swapTotal, 0)}
          </div>
          <Bar value={s.swapTotal ? s.swapUsed / s.swapTotal : 0} thin />
        </div>
      )}
    </Section>
  )
}

function BatterySection({ s }: { s: SystemInfo }) {
  const b = s.battery!
  const charging = b.state === "charging"
  return (
    <Section title="Battery" sub={[b.vendor, b.model].filter(Boolean).join(" ") || undefined}>
      <div className="flex items-center gap-3 py-2">
        {charging ? <BatteryCharging size={20} className="text-ok" /> : <BatteryFull size={20} className="text-accent" />}
        <span className="mono text-[20px] font-medium">{b.percent.toFixed(0)}%</span>
        <span className="text-[12.5px] capitalize text-muted">{b.state}</span>
      </div>
      <KV k="Health" tip={TIPS.health} v={<Badge tone={b.health >= 80 ? "ok" : "warn"}>{b.health.toFixed(0)}%</Badge>} />
      {b.cycles != null && <KV k="Charge cycles" v={b.cycles} />}
      {b.timeToEmptyMins != null && <KV k="Time left" v={duration(b.timeToEmptyMins * 60)} />}
      {b.timeToFullMins != null && <KV k="Until full" v={duration(b.timeToFullMins * 60)} />}
    </Section>
  )
}

function DrivesSection({ s }: { s: SystemInfo | null }) {
  return (
    <Section title="Drives" tip={TIPS.drives} sub={s && `${s.drives.length} mounted`}>
      {!s ? (
        <Loading />
      ) : (
        <div className="grid grid-cols-2 gap-x-7">
          {s.drives.map((d) => {
            const used = d.total ? 1 - d.free / d.total : 0
            return (
              <div key={d.mount} className="flex flex-col gap-1.5 border-t border-border py-2 text-[12.5px]">
                <div className="flex items-center gap-2">
                  <HardDrive size={13} className="shrink-0 text-subtle" />
                  <span className="mono font-medium">{d.mount}</span>
                  <span className="min-w-0 truncate text-subtle">{d.name}</span>
                  <span className="ml-auto shrink-0 text-[11px] text-subtle">{[d.kind, d.fs].filter(Boolean).join(" · ")}</span>
                </div>
                <Bar value={used} thin tone={used > 0.9 ? "warn" : "accent"} />
                <div className="mono flex justify-between text-[11.5px] text-muted">
                  <span>{bytes(d.free, 0)} free</span>
                  <span>{bytes(d.total, 0)}</span>
                </div>
              </div>
            )
          })}
        </div>
      )}
    </Section>
  )
}

function AdminValue({ admin }: { admin: boolean }) {
  const [ask, setAsk] = useState(false)
  if (admin) return <Badge tone="ok">Yes</Badge>
  return (
    <>
      <button type="button" onClick={() => setAsk(true)} className="inline-flex items-center gap-1 text-[12.5px] text-accent hover:underline">
        <ShieldCheck size={13} /> Restart as admin
      </button>
      <RelaunchDialog open={ask} onClose={() => setAsk(false)} />
    </>
  )
}

function Section({ title, sub, tip, children }: { title: string; sub?: string | null | false; tip?: string; children: ReactNode }) {
  return (
    <Card className="flex min-w-0 flex-col px-4 py-3.5">
      <div className="mb-1.5 flex items-center justify-between gap-2">
        <h2 className="m-0 flex shrink-0 items-center text-[13px] font-semibold">
          {title}
          {tip && <InfoTip text={tip} />}
        </h2>
        {sub && <span className="truncate text-[11.5px] text-subtle">{sub}</span>}
      </div>
      {children}
    </Card>
  )
}

/** A label and value. Long values wrap onto a second line instead of being cut. */
function KV({ k, v, tip, wrap }: { k: ReactNode; v: ReactNode; tip?: string; wrap?: boolean }) {
  return (
    <div className="flex min-h-[30px] items-center justify-between gap-3 border-t border-border py-1 text-[12.5px] first-of-type:border-t-0">
      <span className="flex shrink-0 items-center text-muted">
        {k}
        {tip && <InfoTip text={tip} />}
      </span>
      <span className={cn("mono selectable min-w-0 text-right", wrap ? "break-words leading-[1.45]" : "truncate")}>
        {v === undefined || v === null ? <Skeleton className="ml-auto h-3.5 w-20" /> : v}
      </span>
    </div>
  )
}

function Loading() {
  return (
    <div className="flex flex-col gap-2 py-1">
      <Skeleton className="h-4" />
      <Skeleton className="h-4" />
    </div>
  )
}

function ToolSection({ title, tip, tools, cols = 1 }: { title: string; tip: string; tools?: Tool[]; cols?: number }) {
  const installed = tools?.filter((t) => t.installed) ?? []
  const missing = tools?.filter((t) => !t.installed) ?? []
  return (
    <Section title={title} tip={tip} sub={tools && `${installed.length} found`}>
      {!tools ? (
        <Loading />
      ) : (
        <>
          <div className={cn("grid gap-x-7", cols === 2 && "grid-cols-2")}>
            {installed.map((t) => (
              <div key={t.name} className="flex min-h-[30px] items-center justify-between gap-3 border-t border-border text-[12.5px]" title={t.path ?? undefined}>
                <span className="flex min-w-0 items-center gap-2">
                  <BrandIcon name={t.name} size={14} />
                  <span className="truncate">{t.name}</span>
                </span>
                <span className="mono shrink-0 text-right">{t.version ?? <span className="text-subtle">installed</span>}</span>
              </div>
            ))}
          </div>
          {missing.length > 0 && <div className="mt-2 text-[11.5px] leading-[1.6] text-subtle">Not found: {missing.map((t) => t.name).join(", ")}</div>}
        </>
      )}
    </Section>
  )
}

function markdown(s: SystemInfo): string {
  const tools = (ts: Tool[]) => ts.filter((t) => t.installed).map((t) => `- ${t.name}: ${t.version ?? "installed"}`).join("\n")
  return [
    `## System`,
    `- OS: ${s.osVersion} (${s.arch}), kernel ${s.kernel}`,
    `- CPU: ${s.cpuBrand}, ${s.cpuCores} cores / ${s.cpuThreads} threads`,
    `- RAM: ${bytes(s.memTotal)}${s.compression != null ? `, compression ${s.compression ? "on" : "off"}` : ""}`,
    s.gpus.length ? `- GPU: ${s.gpus.join(", ")}` : "",
    s.pageFiles.length ? `- Page file: ${s.pageFiles.map((p) => `${p.path} ${bytes(p.size, 0)}`).join(", ")}` : `- Swap: ${bytes(s.swapTotal)}`,
    s.developerMode != null ? `- Developer Mode: ${s.developerMode ? "on" : "off"}, long paths: ${s.longPaths ? "on" : "off"}, Dev Drive: ${s.devDrive ? "yes" : "no"}` : "",
    s.wsl.length ? `- WSL ${s.wslVersion ?? ""}: ${s.wsl.map((d) => `${d.name} (${d.state})`).join(", ")}` : "",
    s.drives.length ? `- Drives: ${s.drives.map((d) => `${d.mount} ${bytes(d.free, 0)} free of ${bytes(d.total, 0)}`).join(", ")}` : "",
    ``,
    `## Runtimes`,
    tools(s.runtimes),
    ``,
    `## Editors`,
    tools(s.editors),
    ``,
    `## AI coding tools`,
    tools(s.aiTools),
  ]
    .filter((l) => l !== "")
    .join("\n")
}
