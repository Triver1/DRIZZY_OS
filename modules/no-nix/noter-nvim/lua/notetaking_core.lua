-- Markdown-based note-taking with image support

-- Markdown file settings
vim.api.nvim_create_autocmd("FileType", {
	pattern = "markdown",
	callback = function()
		vim.opt_local.number = true
		vim.opt_local.relativenumber = true
		vim.opt_local.conceallevel = 2
		vim.opt_local.wrap = true
		vim.opt_local.linebreak = true
		
		-- Auto-list continuation and indentation
		vim.opt_local.autoindent = true
		vim.opt_local.smartindent = false
		vim.opt_local.shiftwidth = 2
		vim.opt_local.tabstop = 2
		vim.opt_local.expandtab = true
		vim.opt_local.formatoptions:append("r")
		vim.opt_local.formatoptions:append("o")
		vim.opt_local.formatoptions:append("n")
		vim.opt_local.comments = "b:-,b:*,b:+,n:>"
		
		-- Keymaps for easy indentation
		vim.keymap.set("n", ">>", ">>", { buffer = true, desc = "Indent line" })
		vim.keymap.set("n", "<<", "<<", { buffer = true, desc = "Unindent line" })
		vim.keymap.set("v", ">", ">gv", { buffer = true, desc = "Indent selection" })
		vim.keymap.set("v", "<", "<gv", { buffer = true, desc = "Unindent selection" })
	end,
})

return {
	{
		"nvim-treesitter/nvim-treesitter",
		build = ":TSUpdate",
		opts = {
			ensure_installed = { "markdown", "markdown_inline", "lua", "vim" },
			highlight = { enable = true },
		},
		config = function(_, opts)
			require("nvim-treesitter.configs").setup(opts)
		end,
	},
	{
		"3rd/image.nvim",
		rocks = { enabled = false },
		opts = {
			backend = "kitty",
			integrations = {
				markdown = {
					enabled = true,
					clear_in_insert_mode = false,
					only_render_image_at_cursor = false,
				},
			},
			window_overlap_clear_enabled = true,
			window_overlap_clear_ft_ignore = { "cmp_menu", "cmp_docs", "" },
			editor_only_render_when_focused = false,
			tmux_show_only_in_active_window = false,
			hijack_file_patterns = { "*.png", "*.jpg", "*.jpeg", "*.gif", "*.webp" },
		},
	},
	{
		"HakonHarnes/img-clip.nvim",
		event = "VeryLazy",
		opts = {
			default = {
				dir_path = "images",
			},
		},
		keys = {
			{ "<leader>p", "<cmd>PasteImage<cr>", desc = "Paste image from clipboard" },
		},
	},
	{
		"MeanderingProgrammer/render-markdown.nvim",
		opts = {},
		dependencies = { "nvim-treesitter/nvim-treesitter" },
	},
	{
		"nvim-telescope/telescope.nvim",
		branch = "0.1.x",
		dependencies = { "nvim-lua/plenary.nvim" },
		config = function()
			require("telescope").setup({
				pickers = {
					find_files = {
						hidden = true,
					},
				},
			})
		end,
		keys = {
			{ "<leader><leader>", function()
				require("telescope.builtin").find_files({
					prompt_title = "Files & Notes",
					cwd = "~/notes",
					find_command = { "find", ".", "-type", "f", "-name", "*.md" },
				})
			end, desc = "Quick Find Files" },
			{ "<leader>ff", "<cmd>Telescope find_files<cr>", desc = "Find Files" },
			{ "<leader>fg", "<cmd>Telescope live_grep<cr>", desc = "Live Grep" },
			{ "<leader>fb", "<cmd>Telescope buffers<cr>", desc = "Buffers" },
			{ "<leader>fh", "<cmd>Telescope help_tags<cr>", desc = "Help Tags" },
		},
	},
	{
		"nvim-tree/nvim-tree.lua",
		dependencies = { "nvim-tree/nvim-web-devicons" },
		keys = {
			{ "<leader>e", "<cmd>NvimTreeToggle<cr>", desc = "Toggle File Tree" },
		},
		opts = {
			view = {
				width = 30,
			},
			renderer = {
				group_empty = true,
			},
			filters = {
				dotfiles = false,
			},
		},
	},
	{
		"dkarter/bullets.vim",
		ft = { "markdown", "text" },
		config = function()
			vim.g.bullets_enabled_file_types = { "markdown", "text" }
			vim.g.bullets_enable_in_empty_buffers = 0
			vim.g.bullets_outline_levels = { "num", "abc", "std-" }
		end,
	},
	{
		"lukas-reineke/indent-blankline.nvim",
		main = "ibl",
		opts = {
			indent = {
				char = "│",
			},
			scope = {
				enabled = true,
				show_start = true,
				show_end = false,
			},
		},
	},
	{
		"dhruvasagar/vim-table-mode",
		ft = { "markdown", "text" },
		config = function()
			vim.g.table_mode_corner = "|"
			vim.g.table_mode_corner_corner = "|"
			vim.g.table_mode_header_fillchar = "-"
			vim.g.table_mode_motion_up_map = "{<Bar>"
			vim.g.table_mode_motion_down_map = "}<Bar>"
			vim.g.table_mode_motion_left_map = "[<Bar>"
			vim.g.table_mode_motion_right_map = "]<Bar>"
			vim.keymap.set("n", "<leader>tm", "<cmd>TableModeToggle<cr>", { desc = "Toggle Table Mode" })
		end,
	},
	{
		"jakewvincent/mkdnflow.nvim",
		ft = { "markdown" },
		opts = {
			modules = {
				bib = false,
				conceal = false,
				tables = false, -- Disable tables module to fix stack overflow
			},
			links = {
				style = "markdown",
				implicit_extension = "md",
			},
		},
	},
	{
		"dkarter/bullets.vim",
		ft = { "markdown", "text" },
		config = function()
			vim.g.bullets_enabled_file_types = { "markdown", "text" }
			vim.g.bullets_enable_in_empty_buffers = 0
			vim.g.bullets_outline_levels = { "num", "abc", "std-" }
			vim.g.bullets_auto_indent_after_colon = 1
		end,
	},
	{
		"renerocksai/telekasten.nvim",
		ft = { "markdown" },
		opts = {
			home = vim.fn.expand("~/notes"),
			picker = "telescope",
		},
		keys = {
			{ "<leader>zn", "<cmd>Telekasten new_note<cr>", desc = "New note" },
			{ "<leader>zo", "<cmd>Telekasten show_calendar<cr>", desc = "Calendar" },
			{ "<leader>zf", "<cmd>Telekasten find_notes<cr>", desc = "Find notes" },
			{ "<leader>zt", "<cmd>Telekasten find_tags<cr>", desc = "Find tags" },
			{ "<leader>zg", "<cmd>Telekasten search_notes<cr>", desc = "Search notes" },
			{ "<leader>zh", "<cmd>Telekasten follow_link<cr>", desc = "Follow link" },
			{ "<leader>zb", "<cmd>Telekasten show_backlinks<cr>", desc = "Show backlinks" },
			{ "<leader>zl", "<cmd>Telekasten insert_link<cr>", desc = "Insert link" },
		},
	},
}
