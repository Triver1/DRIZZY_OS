-- Bootstrap lazy.nvim
local lazypath = vim.fn.stdpath("data") .. "/lazy/lazy.nvim"
if not (vim.uv or vim.loop).fs_stat(lazypath) then
	local lazyrepo = "https://github.com/folke/lazy.nvim.git"
	local out = vim.fn.system({ "git", "clone", "--filter=blob:none", "--branch=stable", lazyrepo, lazypath })
	if vim.v.shell_error ~= 0 then
		vim.api.nvim_echo({
			{ "Failed to clone lazy.nvim:\n", "ErrorMsg" },
			{ out, "WarningMsg" },
			{ "\nPress any key to exit..." },
		}, true, {})
		vim.fn.getchar()
		os.exit(1)
	end
end
vim.opt.rtp:prepend(lazypath)

-- Set up both the traditional leader (for keymaps) as well as the local leader (for norg files)
vim.g.mapleader = " "
vim.g.maplocalleader = ","

-- Default settings
vim.opt.number = true               -- Show line numbers
vim.opt.relativenumber = true       -- Show relative line numbers
vim.opt.expandtab = true            -- Use spaces instead of tabs
vim.opt.shiftwidth = 2              -- Size of an indent
vim.opt.tabstop = 2                 -- Number of spaces tabs count for
vim.opt.smartindent = true          -- Insert indents automatically
vim.opt.wrap = false                -- Disable line wrap
vim.opt.termguicolors = true        -- True color support
vim.opt.signcolumn = "yes"          -- Always show the signcolumn
vim.opt.updatetime = 250            -- Faster completion
vim.opt.timeoutlen = 300            -- Faster key sequence completion
vim.opt.ignorecase = true           -- Ignore case in search
vim.opt.smartcase = true            -- Don't ignore case with capitals
vim.opt.cursorline = true           -- Highlight current line
vim.opt.scrolloff = 8               -- Lines of context
vim.opt.sidescrolloff = 8           -- Columns of context
vim.opt.smoothscroll = false        -- Disable smooth scrolling
vim.opt.clipboard = "unnamedplus"   -- Use system clipboard

-- Disable all animations
vim.g.snacks_animate = false

-- Window navigation with Ctrl+hjkl
vim.keymap.set("n", "<C-h>", "<C-w>h", { desc = "Move to left window" })
vim.keymap.set("n", "<C-j>", "<C-w>j", { desc = "Move to window below" })
vim.keymap.set("n", "<C-k>", "<C-w>k", { desc = "Move to window above" })
vim.keymap.set("n", "<C-l>", "<C-w>l", { desc = "Move to right window" })

-- Setup lazy.nvim with modular plugin specs
require("lazy").setup({
	spec = {
		{ import = "notetaking_core" },
		{ import = "plugins" },
	},
})

-- Always show dashboard on startup
vim.api.nvim_create_autocmd("VimEnter", {
	callback = function()
		vim.cmd("Alpha")
	end,
})
