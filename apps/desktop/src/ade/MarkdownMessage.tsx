import { Check, Copy } from "@phosphor-icons/react";
import { isValidElement, ReactNode, useState } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";

export function MarkdownMessage({ children }: { children: string }) {
  return (
    <div className="markdown-body">
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        components={{
          a: ({ children: label, ...props }) => (
            <a {...props} target="_blank" rel="noreferrer">
              {label}
            </a>
          ),
          pre: ({ children: code }) => <CodeBlock>{code}</CodeBlock>,
          table: ({ children: table }) => (
            <div className="markdown-table-wrap"><table>{table}</table></div>
          ),
        }}
      >
        {children}
      </ReactMarkdown>
    </div>
  );
}

function CodeBlock({ children }: { children: ReactNode }) {
  const [copied, setCopied] = useState(false);
  const text = textContent(children).replace(/\n$/, "");
  const className = isValidElement<{ className?: string }>(children)
    ? children.props.className ?? ""
    : "";
  const language = className.match(/language-([\w-]+)/)?.[1] ?? "code";

  const copy = async () => {
    await navigator.clipboard.writeText(text);
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1200);
  };

  return (
    <div className="markdown-code-block">
      <div className="markdown-code-head">
        <span>{language}</span>
        <button type="button" onClick={() => void copy()} aria-label="Copy code">
          {copied ? <Check /> : <Copy />}{copied ? "Copied" : "Copy"}
        </button>
      </div>
      <pre>{children}</pre>
    </div>
  );
}

function textContent(node: ReactNode): string {
  if (typeof node === "string" || typeof node === "number") return String(node);
  if (Array.isArray(node)) return node.map(textContent).join("");
  if (isValidElement<{ children?: ReactNode }>(node)) return textContent(node.props.children);
  return "";
}
