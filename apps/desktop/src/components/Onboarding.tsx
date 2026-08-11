import { useState } from "react";
import { DaemonConfig, putConfig } from "../api";

/**
 * Daemon configuration dialog, in two modes:
 *
 * - `welcome` (first run): shown once, when the daemon reports it has never
 *   been configured. Everything is optional — GitHub memory is zero-config
 *   whenever `gh` is authenticated, so "Skip" is a fine answer.
 * - `settings` (header gear): the same form, prefilled with the current
 *   configuration, reachable any time after onboarding.
 *
 * Both report whether the GitHub CLI is ready — with the exact fix when it
 * isn't — and apply saved settings live through `PUT /config`.
 */
export function Onboarding({
  config,
  onDone,
  mode = "welcome",
  onClose,
}: {
  config: DaemonConfig;
  onDone: () => void;
  mode?: "welcome" | "settings";
  onClose?: () => void;
}) {
  const settings = mode === "settings";
  const [backstageUrl, setBackstageUrl] = useState(
    settings ? (config.backstage_base_url ?? "") : "",
  );
  const [backstageToken, setBackstageToken] = useState("");
  const [memoryRepo, setMemoryRepo] = useState(
    settings ? (config.memory_repo ?? "") : "",
  );
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const finish = async (skip: boolean) => {
    setBusy(true);
    setError(null);
    try {
      await putConfig(
        skip
          ? { onboarded: true }
          : {
              backstage_base_url: backstageUrl,
              // Absent = keep the stored token; typing replaces it and an
              // explicit clear is an empty string, which the daemon treats
              // as "no token".
              backstage_token:
                settings && backstageToken === "" ? undefined : backstageToken,
              memory_repo: memoryRepo,
              onboarded: true,
            },
      );
      onDone();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
      setBusy(false);
    }
  };

  const ghReady = config.gh_found && config.gh_authenticated === true;

  return (
    <div className="onboarding-overlay" data-testid="onboarding">
      <div className="onboarding-card">
        <h2>{settings ? "Settings" : "Welcome to OpenADE"}</h2>
        <p className="onboarding-lede">
          {settings
            ? "Changes apply immediately — no restart. Environment variables always take precedence."
            : "30-second setup — everything here is optional and can be changed later. Environment variables always take precedence."}
        </p>

        <div
          className={`onboarding-status ${ghReady ? "ok" : "warn"}`}
          data-testid="ob-gh-status"
        >
          {ghReady && (
            <>
              <strong>✓ GitHub memory is ready.</strong> Sessions
              automatically ground themselves in each repo&rsquo;s GitHub
              remote — nothing to configure.
            </>
          )}
          {config.gh_found && config.gh_authenticated !== true && (
            <>
              <strong>GitHub CLI found, but not signed in.</strong> Run{" "}
              <code>gh auth login</code> in a terminal, then relaunch — or
              continue without GitHub memory.
            </>
          )}
          {!config.gh_found && (
            <>
              <strong>GitHub CLI not found.</strong> Install it from{" "}
              <a href="https://cli.github.com" target="_blank" rel="noreferrer">
                cli.github.com
              </a>{" "}
              and run <code>gh auth login</code> to enable GitHub memory —
              or continue without it.
            </>
          )}
        </div>

        {settings && config.memory_sources.length > 0 && (
          <div className="onboarding-sources" data-testid="ob-sources">
            Active memory sources: {config.memory_sources.join(", ")}
          </div>
        )}

        <div className="form-row">
          <label htmlFor="ob-backstage-url">Backstage URL (optional)</label>
          <input
            id="ob-backstage-url"
            value={backstageUrl}
            onChange={(e) => setBackstageUrl(e.target.value)}
            placeholder="https://backstage.example.com"
            data-testid="ob-backstage-url"
          />
        </div>
        <div className="form-row">
          <label htmlFor="ob-backstage-token">Backstage token (optional)</label>
          <input
            id="ob-backstage-token"
            type="password"
            value={backstageToken}
            onChange={(e) => setBackstageToken(e.target.value)}
            placeholder={
              settings && config.backstage_token_set
                ? "(unchanged — type to replace)"
                : undefined
            }
            data-testid="ob-backstage-token"
          />
        </div>
        <div className="form-row">
          <label htmlFor="ob-memory-repo">
            Shared team memory repo (optional)
          </label>
          <input
            id="ob-memory-repo"
            value={memoryRepo}
            onChange={(e) => setMemoryRepo(e.target.value)}
            placeholder="acme/team-memory"
            data-testid="ob-memory-repo"
          />
          <span className="form-hint">
            A GitHub repo the whole team can push to — session knowledge
            lands there for everyone. Teams can also commit
            <code> .openade/memory-repo</code> per project.
          </span>
        </div>

        {error !== null && (
          <div className="form-error" role="alert">
            {error}
          </div>
        )}

        <div className="form-actions">
          <button
            onClick={() => void finish(false)}
            disabled={busy}
            data-testid="ob-save"
          >
            {busy ? "Saving…" : settings ? "Save" : "Save & start"}
          </button>
          {settings ? (
            <button
              className="secondary"
              onClick={onClose}
              disabled={busy}
              data-testid="ob-cancel"
            >
              Cancel
            </button>
          ) : (
            <button
              className="secondary"
              onClick={() => void finish(true)}
              disabled={busy}
              data-testid="ob-skip"
            >
              Skip for now
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
