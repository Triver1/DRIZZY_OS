-- UI and prettiness plugins
return {
	{
		"rebelot/kanagawa.nvim",
		priority = 1000,
		config = function()
			vim.cmd.colorscheme("kanagawa")
		end,
	},
	{
		"folke/noice.nvim",
		event = "VeryLazy",
		dependencies = {
			"MunifTanjim/nui.nvim",
			"rcarriga/nvim-notify",
		},
		opts = {
			lsp = {
				override = {
					["vim.lsp.util.convert_input_to_markdown_lines"] = true,
					["vim.lsp.util.stylize_markdown"] = true,
					["cmp.entry.get_documentation"] = true,
				},
			},
			presets = {
				bottom_search = true,
				command_palette = true,
				long_message_to_split = true,
				inc_rename = false,
				lsp_doc_border = false,
			},
		},
	},
	{
		"goolord/alpha-nvim",
		dependencies = { "nvim-tree/nvim-web-devicons" },
		config = function()
			local alpha = require("alpha")
			local dashboard = require("alpha.themes.dashboard")
			
			-- Set header
			dashboard.section.header.val = {
				"",
				"  ███╗   ██╗ ██████╗ ████████╗███████╗██████╗ ██╗   ██╗██╗███╗   ███╗",
				"  ████╗  ██║██╔═══██╗╚══██╔══╝██╔════╝██╔══██╗██║   ██║██║████╗ ████║",
				"  ██╔██╗ ██║██║   ██║   ██║   █████╗  ██████╔╝██║   ██║██║██╔████╔██║",
				"  ██║╚██╗██║██║   ██║   ██║   ██╔══╝  ██╔══██╗╚██╗ ██╔╝██║██║╚██╔╝██║",
				"  ██║ ╚████║╚██████╔╝   ██║   ███████╗██║  ██║ ╚████╔╝ ██║██║ ╚═╝ ██║",
				"  ╚═╝  ╚═══╝ ╚═════╝    ╚═╝   ╚══════╝╚═╝  ╚═╝  ╚═══╝  ╚═╝╚═╝     ╚═╝",
				"",
			}
			
			-- Set menu
			dashboard.section.buttons.val = {
				dashboard.button("i", "📋  Open Index", ":e index.md <CR>"),
				dashboard.button("e", "📁  New file", ":ene <BAR> startinsert <CR>"),
				dashboard.button("f", "🔍  Find file", ":Telescope find_files <CR>"),
				dashboard.button("r", "📚  Recent files", ":Telescope oldfiles <CR>"),
				dashboard.button("t", "📝  Find text", ":Telescope live_grep <CR>"),
				dashboard.button("c", "⚙️   Configuration", ":e ~/.config/notevim/init.lua <CR>"),
				dashboard.button("q", "❌  Quit NVim", ":qa<CR>"),
			}
			
			-- Set footer
			dashboard.section.footer.val = {
				"",
				"🎯 NoteVim - Your Personal Knowledge Base",
				"",
			}
			
			-- Set config
			dashboard.config.opts.noautocmd = true
			
			-- Setup
			alpha.setup(dashboard.config)
			
			-- Keymap to open dashboard
			vim.keymap.set("n", "<leader>d", "<cmd>Alpha<cr>", { desc = "Open Dashboard" })
		end,
	},
	{
		"nvim-lualine/lualine.nvim",
		dependencies = { "nvim-tree/nvim-web-devicons" },
		config = function()
			require("lualine").setup({
				options = {
					theme = "kanagawa",
					component_separators = { left = "│", right = "│" },
					section_separators = { left = "", right = "" },
				},
				sections = {
					lualine_a = { "mode" },
					lualine_b = { "branch", "diff", "diagnostics" },
					lualine_c = { "filename" },
					lualine_x = { "encoding", "fileformat", "filetype" },
					lualine_y = { "progress" },
					lualine_z = { "location" },
				},
			})
		end,
	},
	{
		"folke/which-key.nvim",
		event = "VeryLazy",
		opts = {
			preset = "modern",
			delay = 500,
			spec = {
				{ "<leader><leader>", group = "Quick Find Files" },
				{ "<leader>f", group = "Find (Telescope)" },
				{ "<leader>g", group = "Git" },
				{ "<leader>t", group = "Tables" },
				{ "<leader>b", group = "Buffers" },
				{ "<leader>c", group = "Code" },
				{ "<leader>z", group = "Telekasten" },
				{ "<leader>d", group = "Dashboard" },
			},
		},
	},
	{
		"folke/snacks.nvim",
		priority = 1000,
		lazy = false,
		opts = {
			-- Enable beautiful UI components
			bigfile = { enabled = true },
			notifier = { enabled = true },
			quickfile = { enabled = true },
			statuscolumn = { enabled = true },
			words = { enabled = true },
			scroll = { enabled = false },
			animate = { enabled = false },
			indent = {
				enabled = true,
				animate = {
					enabled = false,
				},
			},
			dashboard = {
				enabled = true,
				sections = {
					{ section = "header" },
					{ section = "keys", gap = 1, padding = 1 },
					{ section = "startup" },
				},
			},
		},
		config = function(_, opts)
			local snacks = require("snacks")
			snacks.setup(opts)

			-- Set up some nice keymaps for snacks
			vim.keymap.set("n", "<leader>n", function() snacks.notifier.show_history() end, { desc = "Notification History" })
			vim.keymap.set("n", "<leader>bd", function() snacks.bufdelete() end, { desc = "Delete Buffer" })
			vim.keymap.set("n", "<leader>gg", function() snacks.lazygit() end, { desc = "Lazygit" })
			vim.keymap.set("n", "<leader>gb", function() snacks.git.blame_line() end, { desc = "Git Blame Line" })
			vim.keymap.set("n", "<leader>gB", function() snacks.gitbrowse() end, { desc = "Git Browse" })
			vim.keymap.set("n", "<leader>gf", function() snacks.lazygit.log_file() end, { desc = "Lazygit Current File History" })
			vim.keymap.set("n", "<leader>gl", function() snacks.lazygit.log() end, { desc = "Lazygit Log" })
			vim.keymap.set("n", "<leader>cR", function() snacks.rename() end, { desc = "Rename File" })
		end,
	},
}


