import "@testing-library/jest-dom/vitest";
import { cleanup } from "@testing-library/react";
import { afterEach } from "vitest";

// RTL auto-cleanup only hooks itself up when test globals are enabled;
// we keep globals off, so unmount between tests explicitly.
afterEach(() => {
  cleanup();
});
