import { ChatCircle, Check, Cpu, TerminalWindow } from "@phosphor-icons/react";
import { Preferences, ThemePreference } from "./preferences";

const themes: { id: ThemePreference; name: string; description: string }[] = [
  { id: "graphite", name: "Graphite", description: "Neutral dark IDE palette" },
  { id: "dusk", name: "Dusk", description: "Warm desktop-chat palette" },
  { id: "paper", name: "Paper", description: "Soft light workspace" },
  { id: "glass", name: "Glass", description: "Cool translucent workspace" },
  { id: "system", name: "System", description: "Follow macOS appearance" },
];

export function SettingsPage({ preferences, onChange }: { preferences: Preferences; onChange: (next: Preferences) => void }) {
  const update = <K extends keyof Preferences>(key: K, value: Preferences[K]) => onChange({ ...preferences, [key]: value });
  return <div className="settings-page"><header className="settings-hero"><span className="eyebrow">Workspace preferences</span><h1>Settings</h1><p>Choose how OpenADE looks and where each session starts. These preferences stay local to this machine.</p></header>
    <section className="settings-section"><div className="settings-copy"><h2>Appearance</h2><p>Switch palettes without restarting the daemon or any agent.</p></div><div className="theme-grid">{themes.map((theme) => <button className={preferences.theme === theme.id ? "active" : ""} key={theme.id} onClick={() => update("theme", theme.id)}><span className={`theme-preview preview-${theme.id}`}><i /><i /><i /></span><strong>{theme.name}</strong><small>{theme.description}</small>{preferences.theme === theme.id && <Check />}</button>)}</div></section>
    <section className="settings-section"><div className="settings-copy"><h2>Session experience</h2><p>Set the default surface and how much agent activity stays expanded.</p></div><div className="segmented-settings"><label><span>Start new sessions in</span><div><button className={preferences.session_surface === "chat" ? "active" : ""} onClick={() => update("session_surface", "chat")}><ChatCircle /> Chat + changes</button><button className={preferences.session_surface === "terminal" ? "active" : ""} onClick={() => update("session_surface", "terminal")}><TerminalWindow /> Terminal</button></div></label><label><span>Agent activity</span><div><button className={preferences.activity_detail === "expanded" ? "active" : ""} onClick={() => update("activity_detail", "expanded")}>Expanded thinking</button><button className={preferences.activity_detail === "compact" ? "active" : ""} onClick={() => update("activity_detail", "compact")}>Compact</button></div></label></div></section>
    <section className="settings-section"><div className="settings-copy"><h2>Agent defaults</h2><p>The CLI still uses its own local authentication and configuration.</p></div><label className="setting-select"><Cpu /><span><strong>Default harness</strong><small>Used by the home composer</small></span><select value={preferences.default_agent} onChange={(event) => update("default_agent", event.target.value)}><option value="claude">Claude Code</option><option value="codex">Codex CLI</option><option value="copilot">Copilot CLI</option><option value="opencode">OpenCode</option></select></label></section>
  </div>;
}
