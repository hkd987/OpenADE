import "@testing-library/jest-dom/vitest";
import { cleanup } from "@testing-library/react";
import { afterEach } from "vitest";

// xterm feature-detects canvas support during module evaluation. jsdom's
// placeholder logs a noisy "not implemented" error even though a null context
// is a supported fallback for these component tests.
Object.defineProperty(HTMLCanvasElement.prototype, "getContext", {
  configurable: true,
  value: () => null,
});

// RTL auto-cleanup only hooks itself up when test globals are enabled;
// we keep globals off, so unmount between tests explicitly.
afterEach(() => {
  cleanup();
});
