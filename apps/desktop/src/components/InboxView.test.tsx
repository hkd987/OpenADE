import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { InboxItem, InboxItemDetail } from "../api";
import { InboxView } from "./InboxView";

const { listInbox, getInboxItem, dismissInboxItem, startFromInbox } =
  vi.hoisted(() => ({
    listInbox: vi.fn(),
    getInboxItem: vi.fn(),
    dismissInboxItem: vi.fn(),
    startFromInbox: vi.fn(),
  }));

vi.mock("../api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../api")>()),
  listInbox,
  getInboxItem,
  dismissInboxItem,
  startFromInbox,
}));

const fresh: InboxItem = {
  id: 1,
  fingerprint: "sentry:aa",
  title: "NPE in checkout",
  severity: "critical",
  status: "new",
  created_at: "2026-08-12T00:00:00Z",
  updated_at: "2026-08-12T00:00:00Z",
  affected_count: 12,
  last_seen: "2026-08-12T00:00:00Z",
};

const taken: InboxItem = {
  ...fresh,
  id: 2,
  title: "slow dashboard",
  severity: "medium",
  status: "accepted",
  decided_by: "casey",
  decided_at: "2026-08-12T00:00:00Z",
};

const gone: InboxItem = {
  ...fresh,
  id: 3,
  title: "old noise",
  severity: "low",
  status: "dismissed",
  dismiss_reason: "intended_behavior",
  decided_by: "sam",
};

const detail: InboxItemDetail = {
  item: fresh,
  signals: [
    {
      id: 9,
      source: "sentry",
      source_ref: "E-9",
      kind: "exception",
      severity: "critical",
      title: "NPE in checkout",
      body: "TypeError: cannot read x",
      evidence: [
        { kind: "stack_trace", label: "sentry trace", url: "https://s.example/9" },
      ],
      fingerprint: "sentry:aa",
      join_keys: { release: "v2.3.0" },
      affected_count: 12,
      first_seen: "2026-08-11T00:00:00Z",
      last_seen: "2026-08-12T00:00:00Z",
    },
  ],
  outcomes: [
    {
      item_id: 1,
      kind: "closed",
      occurred_at: "2026-08-01T00:00:00Z",
      pr_url: "https://gh/pr/7",
      note: "first attempt",
    },
  ],
};

describe("InboxView", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    listInbox.mockResolvedValue({ items: [fresh, taken, gone] });
    getInboxItem.mockResolvedValue(detail);
    dismissInboxItem.mockResolvedValue({ ...fresh, status: "dismissed" });
    startFromInbox.mockResolvedValue({ id: "s-1", state: "running" });
  });

  it("groups items by status so the team sees who took what", async () => {
    render(<InboxView repos={[]} onLaunched={vi.fn()} />);
    const rows = await screen.findAllByTestId("inbox-row");
    expect(rows).toHaveLength(3);
    expect(screen.getByText("New")).toBeInTheDocument();
    expect(screen.getByText("In progress")).toBeInTheDocument();
    expect(screen.getByText("Dismissed")).toBeInTheDocument();
    expect(screen.getByTestId("inbox-taken")).toHaveTextContent(
      "Accepted by casey",
    );
    expect(screen.getByTestId("inbox-dismissed")).toHaveTextContent(
      "Dismissed (intended_behavior) by sam",
    );
    expect(rows[0]).toHaveTextContent("affected 12");
  });

  it("shows the empty state and surfaces list errors", async () => {
    listInbox.mockResolvedValue({ items: [] });
    const { unmount } = render(<InboxView repos={[]} onLaunched={vi.fn()} />);
    expect(await screen.findByTestId("inbox-empty")).toHaveTextContent(
      "POST /signals",
    );
    unmount();

    listInbox.mockRejectedValue(new Error("workspace server unreachable"));
    render(<InboxView repos={[]} onLaunched={vi.fn()} />);
    expect(await screen.findByTestId("inbox-error")).toHaveTextContent(
      "unreachable",
    );
  });

  it("opens the one-screen decision view with evidence and prior outcomes", async () => {
    render(<InboxView repos={["/repos/pay"]} onLaunched={vi.fn()} />);
    await userEvent.click((await screen.findAllByTestId("inbox-row"))[0]);

    const view = await screen.findByTestId("inbox-detail");
    expect(view).toHaveTextContent("NPE in checkout");
    expect(view).toHaveTextContent("TypeError: cannot read x");
    const link = screen.getByRole("link", { name: /sentry trace/ });
    expect(link).toHaveAttribute("href", "https://s.example/9");
    const outcomes = screen.getByTestId("inbox-outcomes");
    expect(outcomes).toHaveTextContent("closed");
    expect(outcomes).toHaveTextContent("first attempt");

    await userEvent.click(screen.getByTestId("inbox-back"));
    expect(screen.queryByTestId("inbox-detail")).toBeNull();
  });

  it("detail-load errors surface instead of vanishing", async () => {
    getInboxItem.mockRejectedValue(new Error("detail boom"));
    render(<InboxView repos={[]} onLaunched={vi.fn()} />);
    await userEvent.click((await screen.findAllByTestId("inbox-row"))[0]);
    expect(await screen.findByTestId("inbox-error")).toHaveTextContent(
      "detail boom",
    );
  });

  it("accept starts a triage session; investigate leaves the item undecided", async () => {
    const onLaunched = vi.fn();
    render(<InboxView repos={["/repos/pay"]} onLaunched={onLaunched} />);
    await userEvent.click((await screen.findAllByTestId("inbox-row"))[0]);

    expect(screen.getByTestId("triage-repo")).toHaveValue("/repos/pay");
    await userEvent.selectOptions(
      screen.getByTestId("triage-harness"),
      "opencode",
    );
    await userEvent.click(screen.getByTestId("accept-button"));
    await waitFor(() => expect(onLaunched).toHaveBeenCalled());
    expect(startFromInbox).toHaveBeenCalledWith({
      item_id: 1,
      harness: "opencode",
      repo_root: "/repos/pay",
      investigate: false,
    });

    await userEvent.click(screen.getByTestId("investigate-button"));
    await waitFor(() =>
      expect(startFromInbox).toHaveBeenLastCalledWith(
        expect.objectContaining({ investigate: true }),
      ),
    );
  });

  it("requires a repo and surfaces launch failures", async () => {
    render(<InboxView repos={[]} onLaunched={vi.fn()} />);
    await userEvent.click((await screen.findAllByTestId("inbox-row"))[0]);
    await userEvent.click(screen.getByTestId("accept-button"));
    expect(await screen.findByTestId("inbox-error")).toHaveTextContent(
      "pick a repository",
    );

    startFromInbox.mockRejectedValue(new Error("repository not found"));
    await userEvent.type(screen.getByTestId("triage-repo"), "/nope");
    await userEvent.click(screen.getByTestId("accept-button"));
    expect(await screen.findByTestId("inbox-error")).toHaveTextContent(
      "repository not found",
    );
    expect(screen.getByTestId("accept-button")).toBeEnabled();
  });

  it("dismiss dialog records a structured reason and explains why", async () => {
    render(<InboxView repos={[]} onLaunched={vi.fn()} />);
    await userEvent.click((await screen.findAllByTestId("inbox-row"))[0]);
    await userEvent.click(screen.getByTestId("dismiss-button"));

    const dialog = screen.getByTestId("dismiss-dialog");
    expect(dialog).toHaveTextContent("recorded in outcome memory");
    await userEvent.click(screen.getByTestId("dismiss-wont_fix"));
    await waitFor(() =>
      expect(dismissInboxItem).toHaveBeenCalledWith(1, "wont_fix"),
    );
    expect(screen.queryByTestId("dismiss-dialog")).toBeNull();
    // Back on the list, refreshed.
    expect(listInbox.mock.calls.length).toBeGreaterThan(1);
  });

  it("dismiss failures keep the dialog usable", async () => {
    dismissInboxItem.mockRejectedValue(new Error("already decided"));
    render(<InboxView repos={[]} onLaunched={vi.fn()} />);
    await userEvent.click((await screen.findAllByTestId("inbox-row"))[0]);
    await userEvent.click(screen.getByTestId("dismiss-button"));
    await userEvent.click(screen.getByTestId("dismiss-duplicate"));
    expect(await screen.findByTestId("inbox-error")).toHaveTextContent(
      "already decided",
    );
    await userEvent.click(screen.getByTestId("dismiss-cancel"));
    expect(screen.queryByTestId("dismiss-dialog")).toBeNull();
  });

  it("drives triage from the keyboard: j/k/o/a/d and reason digits", async () => {
    const second: InboxItem = { ...fresh, id: 4, title: "second new" };
    listInbox.mockResolvedValue({ items: [fresh, second] });
    getInboxItem.mockResolvedValue({ ...detail, item: second });
    const onLaunched = vi.fn();
    render(<InboxView repos={["/repos/pay"]} onLaunched={onLaunched} />);
    await screen.findAllByTestId("inbox-row");

    // j moves focus down (clamped), k back up.
    await userEvent.keyboard("j");
    expect(screen.getAllByTestId("inbox-row")[1]).toHaveClass("focused");
    await userEvent.keyboard("j");
    expect(screen.getAllByTestId("inbox-row")[1]).toHaveClass("focused");
    await userEvent.keyboard("k");
    expect(screen.getAllByTestId("inbox-row")[0]).toHaveClass("focused");

    // o opens the focused item; Escape backs out.
    await userEvent.keyboard("j");
    await userEvent.keyboard("o");
    expect(await screen.findByTestId("inbox-detail")).toBeInTheDocument();
    expect(getInboxItem).toHaveBeenCalledWith(4);
    await userEvent.keyboard("{Escape}");
    expect(screen.queryByTestId("inbox-detail")).toBeNull();

    // d opens the dismiss dialog; a digit picks the reason.
    await userEvent.keyboard("d");
    expect(screen.getByTestId("dismiss-dialog")).toBeInTheDocument();
    await userEvent.keyboard("1");
    await waitFor(() =>
      expect(dismissInboxItem).toHaveBeenCalledWith(4, "intended_behavior"),
    );

    // a accepts the focused item with the current defaults.
    await userEvent.keyboard("a");
    await waitFor(() => expect(onLaunched).toHaveBeenCalled());
  });

  it("keyboard ignores modifier combos and typing into fields", async () => {
    render(<InboxView repos={[]} onLaunched={vi.fn()} />);
    await screen.findAllByTestId("inbox-row");

    // ⌘K-style combos never trigger triage actions.
    await userEvent.keyboard("{Meta>}o{/Meta}");
    expect(screen.queryByTestId("inbox-detail")).toBeNull();

    // Typing "d" into the repo field must not open the dismiss dialog.
    await userEvent.click(screen.getAllByTestId("inbox-row")[0]);
    await screen.findByTestId("inbox-detail");
    await userEvent.type(screen.getByTestId("triage-repo"), "d");
    expect(screen.queryByTestId("dismiss-dialog")).toBeNull();

    // Escape inside the dialog cancels without dismissing.
    await userEvent.click(screen.getByTestId("dismiss-button"));
    await userEvent.keyboard("x"); // non-reason key: no-op
    expect(screen.getByTestId("dismiss-dialog")).toBeInTheDocument();
    await userEvent.keyboard("{Escape}");
    expect(screen.queryByTestId("dismiss-dialog")).toBeNull();
    expect(dismissInboxItem).not.toHaveBeenCalled();
  });

  it("decided items show attribution instead of actions", async () => {
    getInboxItem.mockResolvedValue({ ...detail, item: taken });
    render(<InboxView repos={[]} onLaunched={vi.fn()} />);
    await userEvent.click((await screen.findAllByTestId("inbox-row"))[1]);
    expect(await screen.findByTestId("inbox-decided")).toHaveTextContent(
      "Accepted by casey",
    );
    expect(screen.queryByTestId("accept-button")).toBeNull();
  });
});
