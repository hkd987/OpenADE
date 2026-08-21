import { FileCode, Folder, GitDiff, ListMagnifyingGlass } from "@phosphor-icons/react";
import { useEffect, useMemo, useState } from "react";
import { getDiff, getFiles } from "./api";

interface DiffFile {
  path: string;
  status: "A" | "M" | "D";
  lines: string[];
}

export function ReviewWorkspace({ sessionId }: { sessionId: string }) {
  const [diff, setDiff] = useState("");
  const [files, setFiles] = useState<string[]>([]);
  const [scope, setScope] = useState<"changes" | "files">("changes");
  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const changed = useMemo(() => parseUnifiedDiff(diff), [diff]);

  useEffect(() => {
    setError(null);
    void Promise.all([getDiff(sessionId), getFiles(sessionId)])
      .then(([nextDiff, nextFiles]) => { setDiff(nextDiff); setFiles(nextFiles); })
      .catch((reason) => setError(reason instanceof Error ? reason.message : String(reason)));
  }, [sessionId]);

  useEffect(() => {
    if (!selected && changed[0]) setSelected(changed[0].path);
  }, [changed, selected]);

  const current = changed.find((file) => file.path === selected) ?? changed[0];
  const visible = (scope === "changes" ? changed.map((file) => file.path) : files)
    .filter((path) => path.toLowerCase().includes(query.toLowerCase()));
  const status = new Map(changed.map((file) => [file.path, file.status]));

  return (
    <section className="review-workspace">
      <aside className="review-tree">
        <div className="tree-switcher">
          <button className={scope === "changes" ? "active" : ""} onClick={() => setScope("changes")}>Changes <span>{changed.length}</span></button>
          <button className={scope === "files" ? "active" : ""} onClick={() => setScope("files")}>Files <span>{files.length}</span></button>
        </div>
        <label className="tree-search"><ListMagnifyingGlass /><input value={query} onChange={(event) => setQuery(event.target.value)} placeholder="Filter files" /></label>
        <div className="tree-list">
          {visible.map((path) => <FileTreeRow key={path} path={path} status={status.get(path)} active={selected === path} onClick={() => setSelected(path)} />)}
          {visible.length === 0 && <div className="tree-empty">No {scope === "changes" ? "changed" : "indexed"} files</div>}
        </div>
      </aside>
      <div className="diff-view">
        {error ? <div className="inline-error">{error}</div> : current ? <DiffDocument file={current} /> : (
          <div className="panel-empty"><GitDiff /><strong>No uncommitted changes</strong><p>The file tree is ready; agent edits will appear here as a reviewable diff.</p></div>
        )}
      </div>
    </section>
  );
}

function FileTreeRow({ path, status, active, onClick }: { path: string; status?: string; active: boolean; onClick: () => void }) {
  const parts = path.split("/");
  const name = parts.pop() ?? path;
  const directory = parts.join("/");
  return (
    <button className={active ? "active" : ""} onClick={onClick} title={path}>
      {directory ? <Folder /> : <FileCode />}
      <span><strong>{name}</strong>{directory && <small>{directory}</small>}</span>
      {status && <i className={`file-status status-${status.toLowerCase()}`}>{status}</i>}
    </button>
  );
}

function DiffDocument({ file }: { file: DiffFile }) {
  let oldLine = 0;
  let newLine = 0;
  return (
    <article className="diff-document">
      <header><FileCode /><strong>{file.path}</strong><span className={`file-status status-${file.status.toLowerCase()}`}>{file.status}</span></header>
      <div className="diff-lines">
        {file.lines.map((line, index) => {
          if (line.startsWith("@@")) {
            const match = line.match(/@@ -(\d+)(?:,\d+)? \+(\d+)/);
            oldLine = Number(match?.[1] ?? 0);
            newLine = Number(match?.[2] ?? 0);
            return <div className="diff-line hunk" key={`${index}-${line}`}><span /><span /><code>{line}</code></div>;
          }
          const addition = line.startsWith("+") && !line.startsWith("+++");
          const deletion = line.startsWith("-") && !line.startsWith("---");
          const old = addition ? "" : oldLine || "";
          const next = deletion ? "" : newLine || "";
          if (!addition && oldLine) oldLine += 1;
          if (!deletion && newLine) newLine += 1;
          return <div className={`diff-line ${addition ? "addition" : deletion ? "deletion" : "context"}`} key={`${index}-${line}`}><span>{old}</span><span>{next}</span><code>{line || " "}</code></div>;
        })}
      </div>
    </article>
  );
}

export function parseUnifiedDiff(value: string): DiffFile[] {
  if (!value.trim()) return [];
  const files: DiffFile[] = [];
  let current: DiffFile | null = null;
  for (const line of value.replace(/\r/g, "").split("\n")) {
    if (line.startsWith("diff --git ")) {
      if (current) files.push(current);
      const path = line.match(/ b\/(.+)$/)?.[1] ?? "unknown";
      current = { path, status: "M", lines: [] };
      continue;
    }
    if (!current) continue;
    if (line.startsWith("new file mode")) current.status = "A";
    if (line.startsWith("deleted file mode")) current.status = "D";
    if (!line.startsWith("index ") && !line.startsWith("--- ") && !line.startsWith("+++ ")) current.lines.push(line);
  }
  if (current) files.push(current);
  return files;
}
