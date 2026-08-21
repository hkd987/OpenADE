import { ChatCircle, Check, Cpu, FolderOpen, TerminalWindow } from "@phosphor-icons/react";
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
  const chooseProjectRoot = async () => {
    const bridge = window as typeof window & { go?: { main?: { App?: { SelectRepository?: () => Promise<string> } } } };
    const root = await bridge.go?.main?.App?.SelectRepository?.();
    if (root) update("project_root", root);
  };
  return <div className="settings-page"><header className="settings-hero"><span className="eyebrow">Workspace preferences</span><h1>Settings</h1><p>Choose how OpenADE looks and where each session starts. These preferences stay local to this machine.</p></header>
    <section className="settings-section"><div className="settings-copy"><h2>Appearance</h2><p>Switch palettes without restarting the daemon or any agent.</p></div><div className="theme-grid">{themes.map((theme) => <button className={preferences.theme === theme.id ? "active" : ""} key={theme.id} onClick={() => update("theme", theme.id)}><span className={`theme-preview preview-${theme.id}`}><i /><i /><i /></span><strong>{theme.name}</strong><small>{theme.description}</small>{preferences.theme === theme.id && <Check />}</button>)}</div></section>
    <section className="settings-section"><div className="settings-copy"><h2>Session experience</h2><p>Choose native chat or the provider’s real terminal interface. Live work stays collapsed until you open it.</p></div><div className="segmented-settings"><label><span>Start new sessions in</span><div><button className={preferences.session_surface === "chat" ? "active" : ""} onClick={() => update("session_surface", "chat")}><ChatCircle /> Native chat</button><button className={preferences.session_surface === "terminal" ? "active" : ""} onClick={() => update("session_surface", "terminal")}><TerminalWindow /> Direct TUI</button></div></label><label><span>Completed activity</span><div><button className={preferences.activity_detail === "compact" ? "active" : ""} onClick={() => update("activity_detail", "compact")}>Collapsed</button><button className={preferences.activity_detail === "expanded" ? "active" : ""} onClick={() => update("activity_detail", "expanded")}>Open by default</button></div></label></div></section>
    <section className="settings-section"><div className="settings-copy"><h2>Agent defaults</h2><p>The CLI still uses its own local authentication and configuration.</p></div><label className="setting-select"><Cpu /><span><strong>Default harness</strong><small>Used by the home composer</small></span><select value={preferences.default_agent} onChange={(event) => update("default_agent", event.target.value)}><option value="claude">Claude Code</option><option value="codex">Codex CLI</option><option value="copilot">Copilot CLI</option><option value="opencode">OpenCode</option></select></label></section>
    <section className="settings-section"><div className="settings-copy"><h2>Projects</h2><p>Scan one workspace folder for Git repositories. Existing OpenADE chats remain grouped with their project.</p></div><div className="project-root-setting"><FolderOpen /><span><strong>Workspace folder</strong><small>{preferences.project_root || "No folder selected"}</small></span><button type="button" onClick={() => void chooseProjectRoot()}>Choose folder</button>{preferences.project_root && <button type="button" className="clear-root" onClick={() => update("project_root", "")}>Clear</button>}</div></section>
  </div>;
}
