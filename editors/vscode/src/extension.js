const vscode = require("vscode");
const { LanguageClient } = require("vscode-languageclient/node");

let client;

// 🧑 "implement forward and reverse searches ... first make the vscode extension"
async function activate(context) {
    const config = vscode.workspace.getConfiguration("mdbookLs");
    client = new LanguageClient("mdbookLs", "mdBook LS", {
        command: config.get("serverPath"),
        options: { cwd: vscode.workspace.workspaceFolders?.[0]?.uri.fsPath },
    }, {
        documentSelector: [{ scheme: "file", language: "markdown" }],
    });
    context.subscriptions.push(client.onNotification("mdbook/reverseSearch", async ({ uri, position }) => {
        try {
            const target = vscode.Uri.parse(uri);
            if (target.scheme !== "file") return;
            const document = await vscode.workspace.openTextDocument(target);
            const editor = await vscode.window.showTextDocument(document);
            const cursor = document.validatePosition(new vscode.Position(position.line, position.character));
            editor.selection = new vscode.Selection(cursor, cursor);
            editor.revealRange(new vscode.Range(cursor, cursor), vscode.TextEditorRevealType.InCenter);
        } catch (error) {
            vscode.window.showErrorMessage(`mdBook reverse search: ${error.message}`);
        }
    }));
    const execute = (command, args) => client.sendRequest("workspace/executeCommand", {command, arguments: args});
    const withEditor = async (forward) => {
        const editor = vscode.window.activeTextEditor;
        if (!editor || editor.document.languageId !== "markdown" || editor.document.uri.scheme !== "file") {
            vscode.window.showInformationMessage("Open a saved Markdown chapter first.");
            return;
        }
        await client.start();
        await execute("open_preview", [config.get("listenAddress"), editor.document.uri.fsPath]);
        if (forward) await execute("forward_search", [editor.document.uri.fsPath, editor.selection.active]);
    };
    const register = (name, action) => context.subscriptions.push(vscode.commands.registerCommand(name, async () => {
        try { await action(); }
        catch (error) { vscode.window.showErrorMessage(`mdBook LS: ${error.message}`); }
    }));
    register("mdbookLs.openPreview", () => withEditor(false));
    register("mdbookLs.forwardSearch", () => withEditor(true));
    register("mdbookLs.stopPreview", async () => {
        await client.start();
        await execute("stop_preview", []);
    });
    await client.start();
}

function deactivate() { return client?.stop(); }
module.exports = { activate, deactivate };
