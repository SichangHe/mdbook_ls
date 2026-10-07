const assert = require("node:assert/strict");
const vscode = require("vscode");
const WebSocket = require("ws");

async function run() {
    const chapter = vscode.Uri.joinPath(vscode.workspace.workspaceFolders[0].uri, "src/chapter space.md");
    const document = await vscode.workspace.openTextDocument(chapter);
    const editor = await vscode.window.showTextDocument(document);
    const extension = vscode.extensions.getExtension("SichangHe.mdbook-ls");
    await extension.activate();
    assert(extension.isActive);
    await editor.edit(edit => edit.replace(new vscode.Range(0, 0, document.lineCount, 0), "# Chapter\n\nInserted unsaved paragraph.\n\n😀 Unsaved editor content.\n"));
    assert(document.isDirty, "Fixture edit must remain unsaved.");
    editor.selection = new vscode.Selection(4, 2, 4, 2);
    await vscode.commands.executeCommand("mdbookLs.openPreview");
    const deadlineMs = Date.now() + 15000;
    let response;
    while (Date.now() < deadlineMs) {
        response = await fetch("http://127.0.0.1:33019/chapter%20space.html").catch(() => undefined);
        if (response?.ok) break;
        await new Promise(resolve => setTimeout(resolve, 100));
    }
    assert(response, "Preview server did not start.");
    assert(response.ok, `Preview HTTP ${response.status}`);
    await response.text();
    const patchSocket = new WebSocket("ws://127.0.0.1:33019/__mdbook_incremental_preview_live_patch/chapter%20space.html");
    const patch = await new Promise((resolve, reject) => {
        const timer = setTimeout(() => reject(new Error("Initial unsaved buffer was not replayed.")), 10000);
        patchSocket.onmessage = event => {
            const html = event.data.toString();
            if (html.includes("Unsaved editor content.")) { clearTimeout(timer); resolve(html); }
        };
        patchSocket.onerror = reject;
    });
    assert.match(patch, /data-source-line="4"/);
    patchSocket.close();
    const socket = new WebSocket("ws://127.0.0.1:33019/__mdbook_source_search", {origin: "http://127.0.0.1:33019"});
    await new Promise((resolve, reject) => { socket.onopen = resolve; socket.onerror = reject; });
    const forward = new Promise((resolve, reject) => {
        const timer = setTimeout(() => reject(new Error("No forward search websocket message.")), 5000);
        socket.onmessage = event => { clearTimeout(timer); resolve(JSON.parse(event.data)); };
    });
    await vscode.commands.executeCommand("mdbookLs.forwardSearch");
    assert.match((await forward).url, /chapter%20space\.html#mdbook-source-line=4/);
    const reverse = new Promise((resolve, reject) => {
        const timer = setTimeout(() => { listener.dispose(); reject(new Error("No reverse editor navigation.")); }, 5000);
        const listener = vscode.window.onDidChangeTextEditorSelection(event => {
            if (event.textEditor.document.uri.toString() === chapter.toString() && event.selections[0].active.line === 4 && event.selections[0].active.character === 0) {
                clearTimeout(timer); listener.dispose(); resolve();
            }
        });
    });
    socket.send(JSON.stringify({ path: chapter.fsPath, line: 4 }));
    await reverse;
    assert.equal(editor.selection.active.character, 0);
    socket.close();
    await vscode.commands.executeCommand("mdbookLs.stopPreview");
}
module.exports = { run };
