import { useEffect, useRef } from "react";
import { FitAddon } from "@xterm/addon-fit";
import { Terminal } from "@xterm/xterm";
import { resizeTerminal, sendInput, Session, streamURL } from "./api";

export function TerminalPanel({ session }: { session: Session }) {
  const hostRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;

    const terminal = new Terminal({
      allowProposedApi: false,
      convertEol: false,
      cursorBlink: true,
      cursorStyle: "bar",
      fontFamily: '"SFMono-Regular", "JetBrains Mono", Menlo, monospace',
      fontSize: 12.5,
      lineHeight: 1.35,
      scrollback: 10_000,
      theme: {
        background: "#111315",
        foreground: "#d9dddf",
        cursor: "#f2f4f5",
        selectionBackground: "#38566f88",
        black: "#111315",
        red: "#f06a6a",
        green: "#75d196",
        yellow: "#e3b764",
        blue: "#69aee8",
        magenta: "#c59bea",
        cyan: "#76c7c2",
        white: "#e9ecee",
        brightBlack: "#687078",
      },
    });
    const fit = new FitAddon();
    terminal.loadAddon(fit);
    terminal.open(host);

    let socket: WebSocket | undefined;
    let disposed = false;
    let retry: number | undefined;

    const connect = () => {
      if (disposed) return;
      socket = new WebSocket(streamURL(session.id));
      socket.onmessage = (event) => {
        const message = JSON.parse(String(event.data)) as {
          type: string;
          data?: string;
        };
        if (message.type === "output" && message.data) terminal.write(message.data);
      };
      socket.onclose = () => {
        if (!disposed && ["running", "starting", "waiting"].includes(session.status)) {
          retry = window.setTimeout(connect, 900);
        }
      };
    };
    connect();

    const fitAndResize = () => {
      if (disposed || host.clientWidth === 0 || host.clientHeight === 0) return;
      fit.fit();
      void resizeTerminal(session.id, terminal.rows, terminal.cols).catch(() => undefined);
    };
    const observer = new ResizeObserver(fitAndResize);
    observer.observe(host);
    requestAnimationFrame(fitAndResize);

    const input = terminal.onData((data) => {
      void sendInput(session.id, data).catch(() => undefined);
    });

    return () => {
      disposed = true;
      if (retry) window.clearTimeout(retry);
      observer.disconnect();
      input.dispose();
      socket?.close();
      terminal.dispose();
    };
  }, [session.id, session.status]);

  return <div className="terminal-host" ref={hostRef} aria-label="Session terminal" />;
}

