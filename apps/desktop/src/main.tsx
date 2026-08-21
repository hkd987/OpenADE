import React from "react";
import ReactDOM from "react-dom/client";
import AppShell from "./ade/AppShell";
import "./ade/styles.css";
import "@xterm/xterm/css/xterm.css";

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <AppShell />
  </React.StrictMode>,
);
