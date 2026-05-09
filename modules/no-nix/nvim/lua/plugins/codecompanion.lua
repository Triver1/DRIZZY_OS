return {
  {
    "olimorris/codecompanion.nvim",
    version = "^19.0.0",
    dependencies = {
      "nvim-lua/plenary.nvim",
      "nvim-treesitter/nvim-treesitter",
      "ravitemer/codecompanion-history.nvim",
      "sindrets/diffview.nvim",
    },
    opts = {
      adapters = {
        acp = {
          gemini_cli = function()
            return require("codecompanion.adapters").extend("gemini_cli", {
              defaults = {
                auth_method = "oauth-personal",
              },
            })
          end,
          codex = function()
            return require("codecompanion.adapters").extend("codex", {
              defaults = {
                auth_method = "chatgpt",
              },
            })
          end,
        },
      },
      display = {
        diff = {
          enabled = true,
          word_highlights = {
            additions = true,
            deletions = true,
          },
        },
      },
      extensions = {
        history = {
          enabled = true,
          opts = {
            keymap = "gh",
            auto_save = true,
            picker = "snacks",
          },
        },
      },
    },

    config = function(_, opts)
      require("codecompanion").setup(opts)

      -- Diffview setup
      require("diffview").setup({
        enhanced_diff_hl = true,
        use_icons = true,
      })

      -- global keymaps
      vim.keymap.set("n", "<leader>gh", function()
        require("codecompanion-history").open()
      end, { desc = "CodeCompanion History" })

      vim.keymap.set("n", "<leader>gd", "<cmd>DiffviewOpen<cr>", { desc = "Open Diffview" })
      vim.keymap.set("n", "<leader>gD", "<cmd>DiffviewClose<cr>", { desc = "Close Diffview" })
    end,
  },
}
