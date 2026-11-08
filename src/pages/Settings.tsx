import { useState, type ReactNode } from "react"
import { HardDrive, Monitor, Moon, Sun } from "lucide-react"
import { bytes, cn } from "@/lib/format"
import { ACCENTS, useDrives, useSettings, type ThemeMode } from "@/lib/settings"
import { Bar, Card, Checkbox, PageHeader, Skeleton, Switch } from "@/components/ui"
import { PermissionsCard } from "@/components/Privileges"
import { AboutCard } from "@/components/About"

export function SettingsPage() {
  const { settings, set } = useSettings()
  const [portsText, setPortsText] = useState(settings.extraPorts.join(", "))
  const drives = useDrives()

  return (
    <div className="flex max-w-[720px] flex-col gap-4">
      <PageHeader title="Settings" lede="Saved on this machine." />
      <Card className="divide-y divide-border">
        <Row title="Theme" desc="System follows your OS light or dark setting.">
          <div className="flex rounded-lg border border-border-strong p-0.5" role="radiogroup" aria-label="Theme">
            {(
              [
                ["light", "Light", <Sun size={13} key="s" />],
                ["dark", "Dark", <Moon size={13} key="m" />],
                ["system", "System", <Monitor size={13} key="o" />],
              ] as [ThemeMode, string, ReactNode][]
            ).map(([v, label, icon]) => (
              <button
                key={v}
                type="button"
                role="radio"
                aria-checked={settings.theme === v}
                data-testid={`theme-${v}`}
                onClick={() => set("theme", v)}
                className={cn("flex h-7 items-center gap-1.5 rounded-md px-2.5 text-[12.5px] transition-colors", settings.theme === v ? "bg-accent-soft font-medium text-accent" : "text-muted hover:text-text")}
              >
                {icon}
                {label}
              </button>
            ))}
          </div>
        </Row>
        <Row title="Accent colour" desc="Used for buttons, gauges and highlights.">
          <div className="flex items-center gap-2">
            {ACCENTS.map((a) => (
              <button
                key={a.hex}
                type="button"
                aria-label={a.name}
                aria-pressed={settings.accent === a.hex}
                onClick={() => set("accent", a.hex)}
                className="h-5 w-5 rounded-full transition-transform hover:scale-110"
                style={{ background: a.hex, boxShadow: settings.accent === a.hex ? `0 0 0 2px var(--surface), 0 0 0 4px ${a.hex}` : undefined }}
              />
            ))}
            <input type="color" aria-label="Custom accent colour" value={settings.accent} onChange={(e) => set("accent", e.target.value)} className="ml-1 h-6 w-7 cursor-pointer rounded border-0 bg-transparent p-0" />
          </div>
        </Row>
        <Row title="Reduce motion" desc="Turns off animations. The app also follows your OS setting.">
          <Switch checked={settings.reduceMotion} onChange={(v) => set("reduceMotion", v)} label="Reduce motion" />
        </Row>
        <Row title="Confirm before ending safe apps" desc="Apps marked Save first or Unknown always ask.">
          <Switch checked={settings.confirmKills} onChange={(v) => set("confirmKills", v)} label="Confirm before ending safe apps" />
        </Row>
        <Row title="Extra ports to watch" desc="Added to the common dev ports on the Ports page. Comma separated.">
          <input
            value={portsText}
            onChange={(e) => setPortsText(e.target.value)}
            onBlur={() => {
              const ports = portsText
                .split(/[\s,]+/)
                .map(Number)
                .filter((n) => Number.isInteger(n) && n > 0 && n < 65536)
              set("extraPorts", [...new Set(ports)])
              setPortsText(ports.join(", "))
            }}
            placeholder="e.g. 4000, 7070"
            aria-label="Extra ports to watch"
            className="mono h-8 w-[200px] rounded-lg border border-border-strong bg-surface px-2.5 text-[12.5px] outline-none focus:border-accent"
          />
        </Row>
      </Card>
      <Card data-testid="drives-setting">
        <div className="flex items-start justify-between gap-6 px-4 pb-2 pt-3.5">
          <div>
            <div className="text-[13px] font-medium">Drives</div>
            <div className="mt-0.5 text-[12px] text-muted">
              Which drives cleanups include. System junk walks them for unfinished downloads, Steam shader caches and Thumbs.db files and empties their Recycle Bins; Disk cleanup finds the pnpm store on each; AI models searches them for model files; Projects can add them with “Add drives”. Caches in your user folder are always included.
            </div>
          </div>
          <button
            type="button"
            className="shrink-0 text-[12px] text-accent hover:underline"
            onClick={() => set("drives", null)}
          >
            Select all
          </button>
        </div>
        <ul className="m-0 grid list-none grid-cols-2 gap-x-6 px-4 pb-3 pt-1">
          {drives.all.length === 0 &&
            Array.from({ length: 4 }).map((_, i) => (
              <li key={i} className="py-2">
                <Skeleton className="h-8" />
              </li>
            ))}
          {drives.all.map((d) => {
            const on = drives.selected.includes(d.mount)
            return (
              <li key={d.mount} className="flex items-center gap-3 border-t border-border py-2 text-[12.5px]">
                <Checkbox label={`Include ${d.mount}`} checked={on} onChange={(v) => drives.toggle(d.mount, v)} />
                <HardDrive size={14} className="shrink-0 text-subtle" />
                <div className="min-w-0 flex-1">
                  <div className="flex items-center gap-2">
                    <span className="mono font-medium">{d.mount}</span>
                    <span className="truncate text-subtle">{d.name}</span>
                    <span className="ml-auto shrink-0 text-[11px] text-subtle">{d.kind}</span>
                  </div>
                  <Bar value={d.total ? 1 - d.free / d.total : 0} thin className="mt-1" />
                </div>
                <span className="mono w-[86px] shrink-0 text-right text-[11.5px] text-muted">{bytes(d.free, 0)} free</span>
              </li>
            )
          })}
        </ul>
        <div className="flex items-center justify-between gap-6 border-t border-border px-4 py-3.5">
          <div>
            <div className="text-[13px] font-medium">Search these drives for AI models</div>
            <div className="mt-0.5 text-[12px] text-muted">The AI models scan also walks each chosen drive for .gguf, .safetensors and other model files. Slower on big drives.</div>
          </div>
          <Switch checked={settings.modelDrives} onChange={(v) => set("modelDrives", v)} label="Search drives for AI models" />
        </div>
      </Card>
      <PermissionsCard />
      <AboutCard />
      <p className="m-0 text-[12px] text-subtle">Nothing about you or your files leaves this machine. The only network request is the update check, a plain request to GitHub, which you can turn off above.</p>
    </div>
  )
}

function Row({ title, desc, children }: { title: string; desc: string; children: ReactNode }) {
  return (
    <div className="flex items-center justify-between gap-6 px-4 py-3.5">
      <div>
        <div className="text-[13px] font-medium">{title}</div>
        <div className="mt-0.5 text-[12px] text-muted">{desc}</div>
      </div>
      {children}
    </div>
  )
}
