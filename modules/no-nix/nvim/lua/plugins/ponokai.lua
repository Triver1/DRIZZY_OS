return {
  {
    "shawilly/ponokai",
    lazy = false,
    priority = 1000,
    init = function()
      vim.g.ponokai_style = "default"
      vim.g.ponokai_enable_italic = true
      vim.g.ponokai_better_performance = true
    end,
  },
  {
    "LazyVim/LazyVim",
    opts = {
      colorscheme = "ponokai",
    },
  },
}
