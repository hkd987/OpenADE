import { expect, Page, test } from "@playwright/test";
import * as path from "node:path";
import { fileURLToPath } from "node:url";

const currentDir = path.dirname(fileURLToPath(import.meta.url));
const repo = path.resolve(currentDir, ".tmp/fixture-repo");
const daemon = "http://127.0.0.1:7455";

test.describe.configure({ mode: "serial" });

async function trackBrowserResources(page: Page) {
  await page.addInitScript(() => {
    const resources = { sockets: 0, observers: 0 };
    Object.defineProperty(window, "__openADEResources", { value: resources });

    const NativeWebSocket = window.WebSocket;
    class TrackedWebSocket extends NativeWebSocket {
      private counted = false;

      constructor(url: string | URL, protocols?: string | string[]) {
        super(url, protocols);
        if (String(url).includes("/api/")) {
          this.counted = true;
          resources.sockets += 1;
        }
        this.addEventListener("close", () => this.release());
      }

      close(code?: number, reason?: string) {
        this.release();
        super.close(code, reason);
      }

      private release() {
        if (!this.counted) return;
        this.counted = false;
        resources.sockets -= 1;
      }
    }
    window.WebSocket = TrackedWebSocket;

    const NativeResizeObserver = window.ResizeObserver;
    class TrackedResizeObserver implements ResizeObserver {
      private readonly observer: ResizeObserver;
      private counted = false;

      constructor(callback: ResizeObserverCallback) {
        this.observer = new NativeResizeObserver(callback);
      }

      observe(target: Element, options?: ResizeObserverOptions) {
        if (!this.counted) {
          this.counted = true;
          resources.observers += 1;
        }
        this.observer.observe(target, options);
      }

      unobserve(target: Element) {
        this.observer.unobserve(target);
      }

      disconnect() {
        if (this.counted) {
          this.counted = false;
          resources.observers -= 1;
        }
        this.observer.disconnect();
      }
    }
    window.ResizeObserver = TrackedResizeObserver;
  });
}

async function resourceCounts(page: Page) {
  return page.evaluate(() => (
    window as typeof window & { __openADEResources: { sockets: number; observers: number } }
  ).__openADEResources);
}

test("repeated direct-TUI navigation releases sockets and resize observers", async ({ page, request }) => {
  const created = await request.post(`${daemon}/api/sessions`, {
    data: {
      title: "E2E direct TUI lifecycle",
      prompt: "Stay attached for lifecycle verification",
      agent: "codex",
      mode: "tui",
      repo_root: repo,
      base_branch: "main",
    },
  });
  expect(created.status()).toBe(201);
  const session = await created.json() as { id: string };

  await trackBrowserResources(page);
  await page.addInitScript(() => {
    localStorage.setItem("openade.preferences", JSON.stringify({ session_surface: "terminal" }));
  });
  await page.goto("/");
  await expect(page.getByText("Daemon connected")).toBeVisible();

  for (let pass = 0; pass < 5; pass += 1) {
    await page.getByRole("button", { name: /E2E direct TUI lifecycle/ }).first().click();
    await expect(page.getByLabel("Codex TUI in project worktree")).toBeVisible();
    await expect.poll(() => resourceCounts(page)).toEqual({ sockets: 1, observers: 1 });

    await page.getByRole("button", { name: "Back" }).click();
    await expect.poll(() => resourceCounts(page)).toEqual({ sockets: 0, observers: 0 });
  }

  await request.post(`${daemon}/api/sessions/${session.id}/stop`);
});

test("multiple project terminals replace and release their browser resources", async ({ page, request }) => {
  const created = await request.post(`${daemon}/api/sessions`, {
    data: {
      title: "E2E independent terminals",
      prompt: "exec sleep 30",
      agent: "shell",
      mode: "chat",
      repo_root: repo,
      base_branch: "main",
    },
  });
  expect(created.status()).toBe(201);
  const session = await created.json() as { id: string };

  await trackBrowserResources(page);
  await page.goto("/");
  await page.getByRole("button", { name: /E2E independent terminals/ }).first().click();
  await page.getByRole("button", { name: "New terminal" }).first().click();
  await expect(page.getByText("Terminal 1")).toBeVisible();
  await expect.poll(() => resourceCounts(page)).toEqual({ sockets: 1, observers: 1 });

  await page.getByRole("button", { name: "New terminal" }).click();
  await expect(page.getByText("Terminal 2")).toBeVisible();
  await expect.poll(() => resourceCounts(page)).toEqual({ sockets: 1, observers: 1 });

  await page.getByRole("button", { name: "Close Terminal 2", exact: true }).click();
  await expect.poll(() => resourceCounts(page)).toEqual({ sockets: 1, observers: 1 });
  await page.getByRole("button", { name: "Close Terminal 1", exact: true }).click();
  await expect.poll(() => resourceCounts(page)).toEqual({ sockets: 0, observers: 0 });

  await request.post(`${daemon}/api/sessions/${session.id}/stop`);
});
