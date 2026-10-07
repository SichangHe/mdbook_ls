local root = vim.fn.tempname() .. ' book'
vim.fn.mkdir(root .. '/src', 'p')
vim.fn.writefile({ '[book]', 'title = "Editor search"', 'src = "src"' }, root .. '/book.toml')
vim.fn.writefile({ '# Summary', '', '- [Chapter](chapter.md)' }, root .. '/src/SUMMARY.md')
vim.fn.writefile({ '# Heading', '', 'Hello editor', '', 'Last paragraph' }, root .. '/src/chapter.md')
local path = root .. '/src/chapter.md'
local module = dofile('editors/nvim/mdbook_ls.lua')
module.setup({
    cmd = { vim.fn.getcwd() .. '/target/debug/mdbook-ls' },
    address = '127.0.0.1:33020',
})
vim.cmd.edit(vim.fn.fnameescape(path))
vim.bo.filetype = 'markdown'
assert(vim.wait(10000, function()
    local clients = vim.lsp.get_clients({ bufnr = 0, name = 'mdbook_ls' })
    return #clients == 1 and clients[1].initialized
end), 'language server did not attach')
vim.api.nvim_win_set_cursor(0, { 5, 0 })
vim.cmd.MDBookLSForwardSearch()
local reverse_done = false
local browser_error
local browser = [[
(async () => {
    const WebSocket = require(process.argv[2]);
    for (let i = 0; i < 100; i++) {
        try {
            const response = await fetch('http://127.0.0.1:33020/chapter.html');
            if (response.ok) break;
        } catch {}
        if (i === 99) throw new Error('preview never became ready');
        await new Promise(resolve => setTimeout(resolve, 100));
    }
    const ws = new WebSocket('ws://127.0.0.1:33020/__mdbook_source_search', {
        origin: 'http://127.0.0.1:33020',
    });
    await new Promise((resolve, reject) => { ws.onopen = resolve; ws.onerror = reject; });
    ws.send(JSON.stringify({path: process.argv[1], line: 2}));
    await new Promise(resolve => setTimeout(resolve, 250));
    ws.close();
})().catch(error => { console.error(error); process.exitCode = 1; });
]]
vim.system({ 'node', '-e', browser, path, vim.fn.getcwd() .. '/editors/vscode/node_modules/ws' }, { text = true }, function(result)
    browser_error = result.code ~= 0 and result.stderr or nil
    reverse_done = true
end)
assert(vim.wait(15000, function()
    return reverse_done and vim.api.nvim_win_get_cursor(0)[1] == 3
end), browser_error or 'reverse search did not move the Neovim cursor')
assert(not browser_error, browser_error)
vim.cmd.MDBookLSStopPreview()
for _, client in ipairs(vim.lsp.get_clients({ name = 'mdbook_ls' })) do
    client:stop(true)
end
vim.fn.delete(root, 'rf')
vim.cmd.quitall()
