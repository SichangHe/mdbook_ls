const { mkdtempSync, mkdirSync, writeFileSync } = require("node:fs");
const { tmpdir } = require("node:os");
const { join, resolve } = require("node:path");
const { spawnSync } = require("node:child_process");
const { runTests, downloadAndUnzipVSCode, resolveCliArgsFromVSCodeExecutablePath } = require("@vscode/test-electron");
const { name, version } = require("../package.json");

async function main() {
    const fixture = mkdtempSync(join(tmpdir(), "mdbook-vscode-"));
    mkdirSync(join(fixture, "src"));
    writeFileSync(join(fixture, "book.toml"), '[book]\ntitle = "Editor test"\n');
    writeFileSync(join(fixture, "src", "SUMMARY.md"), '# Summary\n\n- [Chapter](<chapter space.md>)\n');
    writeFileSync(join(fixture, "src", "chapter space.md"), '# Chapter\n\n😀 Hello editor.\n');
    const userData = mkdtempSync(join(tmpdir(), "mdbook-vscode-user-"));
    mkdirSync(join(userData, "User"));
    writeFileSync(join(userData, "User", "settings.json"), JSON.stringify({
        "mdbookLs.serverPath": process.env.MDBOOK_LS_SERVER || resolve(__dirname, "../../../target/debug/mdbook-ls"),
        "mdbookLs.listenAddress": "127.0.0.1:33019",
    }));
    const vscodeExecutablePath = await downloadAndUnzipVSCode();
    const extensionsDir = mkdtempSync(join(tmpdir(), "mdbook-vscode-extensions-"));
    const [cli, ...cliArgs] = resolveCliArgsFromVSCodeExecutablePath(vscodeExecutablePath);
    const install = spawnSync(cli, [...cliArgs, `--extensions-dir=${extensionsDir}`, `--user-data-dir=${userData}`, "--install-extension", resolve(__dirname, "..", `${name}-${version}.vsix`)], {stdio: "inherit"});
    if (install.status !== 0) throw new Error("VSIX installation failed.");
    await runTests({
        vscodeExecutablePath,
        extensionDevelopmentPath: resolve(__dirname, "empty_extension"),
        extensionTestsPath: resolve(__dirname, "suite.js"),
        launchArgs: [fixture, `--user-data-dir=${userData}`, `--extensions-dir=${extensionsDir}`, "--no-sandbox", "--disable-gpu", "--disable-workspace-trust"],
    });
}
main().catch(error => { console.error(error); process.exit(1); });
