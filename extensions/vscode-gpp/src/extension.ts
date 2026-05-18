// vscode-gpp — thin VS Code surface over the `gpp` CLI.
//
// It shells out to `gpp --json …` for data (the CLI is the single source of
// truth) and renders Timeline / Graphex / Reviews tree views, plus commands
// for promote and semantic diff. Native MCP context injection rides on the
// `gpp mcp-server --stdio` server configured as an MCP provider.
import * as vscode from "vscode";
import { execFile } from "node:child_process";

function gpp(args: string[], cwd: string): Promise<string> {
  return new Promise((resolve, reject) => {
    execFile("gpp", args, { cwd, maxBuffer: 8 * 1024 * 1024 }, (err, stdout) => {
      if (err) reject(err);
      else resolve(stdout);
    });
  });
}

class LinesProvider implements vscode.TreeDataProvider<string> {
  private _onDidChange = new vscode.EventEmitter<void>();
  readonly onDidChangeTreeData = this._onDidChange.event;
  constructor(private readonly args: string[], private readonly cwd: string) {}
  refresh() { this._onDidChange.fire(); }
  getTreeItem(e: string) { return new vscode.TreeItem(e); }
  async getChildren(): Promise<string[]> {
    try {
      return (await gpp(this.args, this.cwd)).split("\n").filter((l) => l.trim());
    } catch (e) {
      return [`(gpp error: ${e})`];
    }
  }
}

export function activate(ctx: vscode.ExtensionContext) {
  const root = vscode.workspace.workspaceFolders?.[0].uri.fsPath ?? ".";
  const timeline = new LinesProvider(["timeline", "-n", "50"], root);
  const graphex = new LinesProvider(["graphex", "status"], root);
  const reviews = new LinesProvider(["review", "list"], root);

  vscode.window.registerTreeDataProvider("gpp.timeline", timeline);
  vscode.window.registerTreeDataProvider("gpp.graphex", graphex);
  vscode.window.registerTreeDataProvider("gpp.reviews", reviews);

  ctx.subscriptions.push(
    vscode.commands.registerCommand("gpp.refresh", () => {
      timeline.refresh();
      graphex.refresh();
      reviews.refresh();
    }),
    vscode.commands.registerCommand("gpp.promote", async () => {
      const msg = await vscode.window.showInputBox({ prompt: "Changeset message" });
      if (!msg) return;
      await gpp(["promote", "-m", msg], root);
      vscode.window.showInformationMessage("gpp: promoted");
      timeline.refresh();
    }),
    vscode.commands.registerCommand("gpp.diffSemantic", async () => {
      const out = await gpp(["diff", "HEAD", "--semantic"], root);
      const doc = await vscode.workspace.openTextDocument({ content: out, language: "diff" });
      vscode.window.showTextDocument(doc);
    }),
  );
}

export function deactivate() {}
