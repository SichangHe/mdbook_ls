local M = {}

local function execute(command, arguments)
    for _, client in ipairs(vim.lsp.get_clients({ bufnr = 0, name = 'mdbook_ls' })) do
        client:request('workspace/executeCommand', {
            command = command,
            arguments = arguments,
        }, function(err)
            if err then
                vim.notify(err.message, vim.log.levels.ERROR)
            end
        end, 0)
    end
end

-- 🧑 "implement forward and reverse searches and integrate with nvim and vscode"
function M.setup(opts)
    opts = opts or {}
    local address = opts.address or '127.0.0.1:33000'
    vim.lsp.config('mdbook_ls', {
        cmd = opts.cmd or { 'mdbook-ls' },
        filetypes = { 'markdown' },
        root_markers = { 'book.toml' },
        handlers = {
            ['mdbook/reverseSearch'] = function(err, params)
                if err or not params then
                    return
                end
                vim.lsp.util.show_document({
                    uri = params.uri,
                    range = { start = params.position, ['end'] = params.position },
                }, 'utf-16', { focus = true })
            end,
        },
    })
    vim.lsp.enable('mdbook_ls')
    vim.api.nvim_create_user_command('MDBookLSOpenPreview', function()
        execute('open_preview', { address, vim.api.nvim_buf_get_name(0) })
    end, {})
    vim.api.nvim_create_user_command('MDBookLSForwardSearch', function()
        local position = vim.lsp.util.make_position_params(0, 'utf-16').position
        execute('open_preview', { address, vim.api.nvim_buf_get_name(0) })
        execute('forward_search', { vim.api.nvim_buf_get_name(0), position })
    end, {})
    vim.api.nvim_create_user_command('MDBookLSStopPreview', function()
        execute('stop_preview', {})
    end, {})
end

return M
