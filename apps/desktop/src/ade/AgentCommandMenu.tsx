import { Command, Lightning } from "@phosphor-icons/react";
import { AgentCommand } from "./api";

export function filterAgentCommands(commands: AgentCommand[], input: string): AgentCommand[] {
  const token = input.trimStart().split(/\s+/)[0] ?? "";
  const query = token.replace(/^[/ $]+/, "").toLowerCase();
  return commands
    .filter((command) => !query || `${command.name} ${command.description ?? ""}`.toLowerCase().includes(query))
    .slice(0, 8);
}

export function AgentCommandMenu({ commands, input, onSelect }: { commands: AgentCommand[]; input: string; onSelect: (command: AgentCommand) => void }) {
  const visible = filterAgentCommands(commands, input);
  return <div className="agent-command-menu" role="listbox" aria-label="Skills and commands">
    <header><span>Skills & commands</span><small>{visible.length} available</small></header>
    <div>
      {visible.map((command, index) => <button type="button" role="option" aria-selected={index === 0} key={command.id} onMouseDown={(event) => event.preventDefault()} onClick={() => onSelect(command)}>
        <span className={`command-kind ${command.kind}`}>{command.kind === "skill" ? <Lightning /> : <Command />}</span>
        <span><strong>{command.invocation}</strong><small>{command.description || `${command.source} ${command.kind}`}</small></span>
        <i>{command.source}</i>
      </button>)}
      {visible.length === 0 && <p>No matching skills or commands.</p>}
    </div>
    <footer><span>Enter inserts the first match</span><kbd>esc</kbd><span>close</span></footer>
  </div>;
}
