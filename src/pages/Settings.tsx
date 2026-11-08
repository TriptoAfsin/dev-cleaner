import { useState, type ReactNode } from "react"
import { Monitor, Moon, Sun } from "lucide-react"
import { cn } from "@/lib/format"
import { ACCENTS, useSettings, type ThemeMode } from "@/lib/settings"
import { Card, PageHeader, Switch } from "@/components/ui"

export function SettingsPage() {
  const { settings, set } = useSettings()
  const [portsText, setPortsText] = useState(settings.extraPorts.join(", "))

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
      <p className="m-0 text-[12px] text-subtle">Dev Cleaner never sends anything off your machine.</p>
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
