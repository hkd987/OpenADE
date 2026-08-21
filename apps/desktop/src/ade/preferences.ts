export type ThemePreference = "graphite" | "dusk" | "paper" | "glass" | "system";
export type SessionSurface = "chat" | "terminal";
export type ActivityDetail = "compact" | "expanded";

export interface Preferences {
  theme: ThemePreference;
  default_agent: string;
  session_surface: SessionSurface;
  activity_detail: ActivityDetail;
}

export const defaultPreferences: Preferences = {
  theme: "graphite",
  default_agent: "claude",
  session_surface: "chat",
  activity_detail: "expanded",
};

export function loadPreferences(): Preferences {
  try {
    return { ...defaultPreferences, ...JSON.parse(localStorage.getItem("openade.preferences") ?? "{}") };
  } catch {
    return defaultPreferences;
  }
}

export function savePreferences(preferences: Preferences) {
  localStorage.setItem("openade.preferences", JSON.stringify(preferences));
}

export function themeClass(theme: ThemePreference): string {
  if (theme === "paper") return "theme-light";
  if (theme === "dusk") return "theme-dusk";
  if (theme === "glass") return "theme-glass";
  if (theme === "system") return "theme-system";
  return "theme-dark";
}
